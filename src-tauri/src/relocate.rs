//! 同迭代排序与跨迭代移动（设计 014）：
//! 同迭代只重排清单数组；跨迭代真实执行 `git worktree move`（脏工作区只用单个 `-f`）并先写目标清单、再写源清单。

use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner};
use crate::models::{AppConfig, Manifest, ManifestHealth, ManifestProject, MoveOutcome, MoveProjectRequest};
use crate::{manifest, path_utils, platform, validation};

/// 同迭代重排：把 `worktree_path` 对应记录移到 `before` 之前（`None` ＝ 末尾）
pub fn reorder(
    config: &AppConfig,
    iteration: &str,
    worktree_path: &str,
    before: Option<&str>,
) -> Result<(), AppError> {
    let iteration_dir = source_iteration_directory(config, iteration)?;
    let mut data = load_manifest_for_relocate(&iteration_dir)?;

    let index = data
        .projects
        .iter()
        .position(|record| platform::paths_equal(Path::new(&record.worktree_path), Path::new(worktree_path)))
        .ok_or_else(|| AppError::NotFound("清单中找不到该 worktree 记录".to_string()))?;

    match before {
        Some(before_path) => {
            if platform::paths_equal(Path::new(before_path), Path::new(worktree_path)) {
                return Ok(()); // 落点是自身 → 原位，不发请求也不写盘
            }
            let anchor = data
                .projects
                .iter()
                .position(|record| platform::paths_equal(Path::new(&record.worktree_path), Path::new(before_path)))
                .ok_or_else(|| AppError::NotFound("锚点记录不存在".to_string()))?;
            if anchor == index + 1 {
                return Ok(()); // 落点是自己的下一条 → 原位
            }
        }
        None => {
            if index + 1 == data.projects.len() {
                return Ok(()); // 追加到末尾且已在末尾 → 原位
            }
        }
    }

    let record = data.projects.remove(index);
    match before {
        None => data.projects.push(record),
        Some(before_path) => {
            let insert = data
                .projects
                .iter()
                .position(|item| platform::paths_equal(Path::new(&item.worktree_path), Path::new(before_path)))
                .expect("锚点记录仍应存在");
            data.projects.insert(insert, record);
        }
    }
    manifest::write_manifest(&iteration_dir, &data)
}

struct MovePlan {
    repository: PathBuf,
    old_path: PathBuf,
    new_path: PathBuf,
    dirty: bool,
    insert_at: usize,
    record_index: usize,
    record: ManifestProject,
    source_manifest: Manifest,
    target_manifest: Manifest,
    source_dir: PathBuf,
    target_dir: PathBuf,
}

/// 跨迭代移动：`git worktree move` 搬运本体，成功后先写目标清单、再写源清单
pub fn move_project(config: &AppConfig, request: &MoveProjectRequest) -> Result<MoveOutcome, AppError> {
    let plan = plan_move(config, request)?;
    let runner = GitRunner::new(&plan.repository);
    // 脏工作区只用单个 -f（禁止 -f -f）
    git::run(&runner.cmd_worktree_move(&plan.old_path, &plan.new_path, plan.dirty))?;

    let mut target = plan.target_manifest.clone();
    let mut record = plan.record.clone();
    record.worktree_path = plan.new_path.to_string_lossy().to_string();
    target.projects.insert(plan.insert_at, record);
    manifest::write_manifest(&plan.target_dir, &target).map_err(|error| {
        AppError::Io(format!(
            "worktree 已搬到 {}，但目标迭代清单写入失败：{}。请点击「复核状态」（该 worktree 将以 discovered 形式出现）或手工修正两份清单。",
            plan.new_path.display(),
            error.message()
        ))
    })?;

    let mut source = plan.source_manifest.clone();
    source.projects.remove(plan.record_index);
    manifest::write_manifest(&plan.source_dir, &source).map_err(|error| {
        AppError::Io(format!(
            "worktree 已搬到 {} 且已登记到目标迭代，但源迭代清单未能删除旧记录：{}。请点击「复核状态」（旧记录将显示目录缺失）或手工修正源清单。",
            plan.new_path.display(),
            error.message()
        ))
    })?;

    Ok(MoveOutcome {
        iteration: request.iteration.clone(),
        target_iteration: request.target_iteration.clone(),
        project_id: plan.record.project_id.clone(),
        worktree_path: plan.old_path.to_string_lossy().to_string(),
        target_worktree_path: plan.new_path.to_string_lossy().to_string(),
        target_directory: plan
            .new_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    })
}

/// 预检（任一不通过即 `Err`，**不改动任何内容**）
fn plan_move(config: &AppConfig, request: &MoveProjectRequest) -> Result<MovePlan, AppError> {
    // 1) 迭代号合法且不相同
    validation::validate_iteration(&request.iteration)?;
    validation::validate_iteration(&request.target_iteration)?;
    if request.iteration == request.target_iteration {
        return Err(AppError::Validation(
            "目标迭代与源迭代相同，请使用同迭代排序".to_string(),
        ));
    }

    let source_dir = source_iteration_directory(config, &request.iteration)?;
    let target_dir = iteration_directory(config, &request.target_iteration)?;

    // 2) 两份清单都必须 valid
    let source_manifest = load_manifest_for_relocate(&source_dir)?;
    let target_manifest = match manifest::read_manifest(&target_dir).health {
        ManifestHealth::Valid => manifest::load_manifest_strict(&target_dir)?,
        ManifestHealth::Missing => {
            return Err(AppError::NotFound(
                "目标迭代清单不存在（目标迭代必须已由本工具创建）".to_string(),
            ))
        }
        ManifestHealth::Damaged => {
            return Err(AppError::ManifestDamaged(format!(
                "目标迭代清单损坏：{}",
                target_dir.display()
            )))
        }
    };

    // 3) 目标迭代未归档
    if target_manifest.archived_at.is_some() {
        return Err(AppError::Conflict("目标迭代已归档".to_string()));
    }

    // 4) 源记录存在且 active
    let record_index = source_manifest
        .projects
        .iter()
        .position(|record| {
            platform::paths_equal(Path::new(&record.worktree_path), Path::new(&request.worktree_path))
        })
        .ok_or_else(|| AppError::NotFound("清单中找不到该 worktree 记录".to_string()))?;
    let record = source_manifest.projects[record_index].clone();
    if record.lifecycle != crate::models::Lifecycle::Active {
        return Err(AppError::Conflict(format!(
            "该记录不是 active（当前 {}），无法移动",
            match record.lifecycle {
                crate::models::Lifecycle::Active => "active",
                crate::models::Lifecycle::Removed => "removed",
                crate::models::Lifecycle::CreateFailed => "createFailed",
            }
        )));
    }

    // 5) 目录名匹配 projectId 或 projectId-N
    let directory = manifest::directory_name_of(Path::new(&record.worktree_path));
    if !matches_project_directory(&record.project_id, &directory) {
        return Err(AppError::Conflict(format!(
            "目录名「{directory}」不符合托管规则（应为 {} 或 {}-N）",
            record.project_id, record.project_id
        )));
    }

    // 6) 源目录存在且在源迭代目录内
    let old_path = path_utils::canonicalize_existing(Path::new(&record.worktree_path))?;
    path_utils::ensure_within(&source_dir, &old_path)?;

    // 7-9) 源仓库注册 / locked / prunable
    let repository = PathBuf::from(&record.source_repository);
    if !repository.is_dir() {
        return Err(AppError::Validation(format!(
            "源仓库不存在：{}",
            record.source_repository
        )));
    }
    let runner = GitRunner::new(&repository);
    let entries = git::run(&runner.cmd_worktree_list_porcelain())
        .map(|output| git::parse_worktree_list(&output))?;
    let Some(entry) = entries
        .iter()
        .find(|entry| platform::paths_equal(&entry.path, &old_path))
    else {
        return Err(AppError::Conflict("worktree 未在源仓库注册".to_string()));
    };
    if entry.locked {
        return Err(AppError::Conflict("worktree 已被锁定".to_string()));
    }
    if entry.prunable {
        return Err(AppError::Conflict(
            "worktree 处于 prunable 状态".to_string(),
        ));
    }

    // 10) 目标目录名顺延（占用集合含目标清单全部记录 + 目标迭代公共目录目标名）
    let occupied = manifest::occupied_names(&target_manifest);
    let target_directory = path_utils::next_directory_name(&occupied, &record.project_id);
    let new_path = path_utils::join_segments(&target_dir, &[&target_directory])?;
    // 11) 目标路径必须不存在
    if new_path.exists() {
        return Err(AppError::Conflict(format!(
            "目标路径已存在：{}",
            new_path.display()
        )));
    }

    // 12) 锚点可定位
    let insert_at = match request.before_worktree_path.as_deref() {
        None => target_manifest.projects.len(),
        Some(before_path) => target_manifest
            .projects
            .iter()
            .position(|item| {
                platform::paths_equal(Path::new(&item.worktree_path), Path::new(before_path))
            })
            .ok_or_else(|| AppError::NotFound("目标迭代中找不到锚点记录".to_string()))?,
    };

    // 13) 脏工作区 → 单个 -f
    let status = git::run(&GitRunner::new(&old_path).cmd_status_porcelain())?;
    let dirty = git::parse_status(&status).is_dirty();

    Ok(MovePlan {
        repository,
        old_path,
        new_path,
        dirty,
        insert_at,
        record_index,
        record,
        source_manifest,
        target_manifest,
        source_dir,
        target_dir,
    })
}

fn matches_project_directory(project_id: &str, directory: &str) -> bool {
    if directory == project_id {
        return true;
    }
    match directory.strip_prefix(&format!("{project_id}-")) {
        Some(suffix) => {
            !suffix.is_empty()
                && suffix.chars().all(|ch| ch.is_ascii_digit())
                && suffix.parse::<u32>().map(|value| value >= 2).unwrap_or(false)
        }
        None => false,
    }
}

fn iteration_directory(config: &AppConfig, iteration: &str) -> Result<PathBuf, AppError> {
    validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::canonicalize_existing(&path_utils::resolve_allow_missing(Path::new(root))?)?;
    let raw = path_utils::join_segments(&root, &[iteration])?;
    let directory = raw
        .canonicalize()
        .map_err(|_| AppError::NotFound(format!("迭代目录不存在：{}", raw.display())))?;
    path_utils::ensure_within(&root, &directory)?;
    Ok(directory)
}

/// 源迭代目录：目录或清单缺失都按 `ManifestDamaged` 处理（设计 014 §3.2 第 1-2 步）
fn source_iteration_directory(config: &AppConfig, iteration: &str) -> Result<PathBuf, AppError> {
    iteration_directory(config, iteration).map_err(|error| {
        if error.code() == crate::models::ErrorCode::NotFound {
            AppError::ManifestDamaged(format!(
                "源迭代清单不存在（迭代目录缺失）：{}",
                error.message()
            ))
        } else {
            error
        }
    })
}

/// 排序 / 移动要求清单可读：`missing` 与 `damaged` 都返回 `ManifestDamaged`（设计 014 §3.2）
fn load_manifest_for_relocate(iteration_dir: &Path) -> Result<Manifest, AppError> {
    match manifest::read_manifest(iteration_dir).health {
        ManifestHealth::Valid => manifest::load_manifest_strict(iteration_dir),
        ManifestHealth::Missing => Err(AppError::ManifestDamaged(format!(
            "迭代清单不存在：{}",
            manifest::manifest_path(iteration_dir).display()
        ))),
        ManifestHealth::Damaged => Err(AppError::ManifestDamaged(format!(
            "迭代清单损坏：{}",
            iteration_dir.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_directory_rule() {
        assert!(matches_project_directory("api3", "api3"));
        assert!(matches_project_directory("api3", "api3-2"));
        assert!(matches_project_directory("api3", "api3-10"));
        assert!(!matches_project_directory("api3", "api3-1"));
        assert!(!matches_project_directory("api3", "api3-x"));
        assert!(!matches_project_directory("api3", "api30"));
        assert!(!matches_project_directory("api3", "api3-2-extra"));
    }
}

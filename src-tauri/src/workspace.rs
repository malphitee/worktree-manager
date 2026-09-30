//! 创建编排（requirements.md 判定口径 1-4；workflows.md §2）：
//! 清单初始化 → 公共目录全部完成 → 逐项目串行 fetch / worktree add / vendor → 每项目后原子写清单。
//! 单项目失败不影响后续项目，也不回滚已成功项目；公共目录失败则整批终止且不开始 fetch。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner, WorktreeEntry};
use crate::models::{
    AppConfig, CreateBatchResult, CreatePhase, CreateProgress, CreateProjectRequest,
    CreateProjectResult, CreateRequest, CreateResult, CreateStatus, HeadMode, Lifecycle, Manifest,
    ManifestHealth, ManifestProject, PostStepRecord, PostStepStatus, SharedDirRecord,
    SharedDirStatus, VendorRecord, VendorStatus, SCHEMA_VERSION,
};
use crate::path_utils;
use crate::platform;
use crate::validation;
use crate::{base_ref, config as app_config, copy, manifest, vendor};

/// 备注规范化：去首尾空白、≤50 个 Unicode 标量、单行；空串＝清除（`None`）
pub fn normalize_note(text: &str) -> Result<Option<String>, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(AppError::Validation("备注不能换行".to_string()));
    }
    if trimmed.chars().count() > 50 {
        return Err(AppError::Validation(
            "备注不能超过 50 字符".to_string(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// 创建编排入口；`emit` 用于发送 `create-progress` 载荷。
pub fn create_batch<F>(
    config: &AppConfig,
    request: CreateRequest,
    emit: F,
) -> Result<CreateBatchResult, AppError>
where
    F: Fn(CreateProgress),
{
    validation::validate_iteration(&request.iteration)?;
    let Some(root_raw) = config.workspace_root.clone() else {
        return Err(AppError::Validation(
            "请先在设置中填写工作区根目录".to_string(),
        ));
    };

    // 工作区根目录允许尚不存在：解析 → 创建 → canonicalize
    let root = path_utils::resolve_allow_missing(Path::new(&root_raw))?;
    fs::create_dir_all(&root)
        .map_err(|error| AppError::Io(format!("创建工作区根目录失败 {}：{error}", root.display())))?;
    let root = path_utils::canonicalize_existing(&root)?;
    let iteration_dir = path_utils::join_segments(&root, &[&request.iteration])?;
    fs::create_dir_all(&iteration_dir).map_err(|error| {
        AppError::Io(format!(
            "创建迭代目录失败 {}：{error}",
            iteration_dir.display()
        ))
    })?;
    let iteration_dir = path_utils::canonicalize_existing(&iteration_dir)?;
    let iteration_path = iteration_dir.to_string_lossy().to_string();

    // 清单：missing → 新建；valid → 复用；damaged → 报错不写盘；已归档 → conflict
    let read = manifest::read_manifest(&iteration_dir);
    let mut data = match read.health {
        ManifestHealth::Valid => read.manifest.expect("valid 清单必然可解析"),
        ManifestHealth::Missing => Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: request.iteration.clone(),
            created_at: crate::models::now_rfc3339(),
            note: None,
            hidden_at: None,
            archived_at: None,
            shared_directories: Vec::new(),
            projects: Vec::new(),
        },
        ManifestHealth::Damaged => {
            return Err(AppError::ManifestDamaged(
                read.message.unwrap_or_else(|| "清单损坏".to_string()),
            ))
        }
    };
    if data.archived_at.is_some() {
        return Err(AppError::Conflict("该迭代已归档".to_string()));
    }

    // 备注三态（缺省＝不改动）与自动取消隐藏
    match request.note.clone() {
        None => {}
        Some(None) => data.note = None,
        Some(Some(text)) => data.note = normalize_note(&text)?,
    }
    if data.hidden_at.is_some() {
        data.hidden_at = None;
    }
    manifest::write_manifest(&iteration_dir, &data)?;

    // recentIterations：本批次仅一次原子写
    let config = app_config::remember_iteration(config.clone(), &request.iteration)?;

    // 公共目录先行：任一条失败 → 记录 failed、整批终止、不开始任何 fetch
    let mut aborted = false;
    let mut abort_reason: Option<String> = None;
    for rule in &config.shared_directories {
        let target = path_utils::join_segments(&iteration_dir, &[&rule.target_directory])?;
        let position = data
            .shared_directories
            .iter()
            .position(|record| record.rule_id == rule.target_directory);
        let managed_copied = position
            .map(|index| {
                matches!(
                    data.shared_directories[index].status,
                    SharedDirStatus::Copied | SharedDirStatus::Reused
                ) && target.is_dir()
            })
            .unwrap_or(false);

        if managed_copied {
            if let Some(index) = position {
                data.shared_directories[index].status = SharedDirStatus::Reused;
                data.shared_directories[index].message = None;
                data.shared_directories[index].target_path = target.to_string_lossy().to_string();
            }
            manifest::write_manifest(&iteration_dir, &data)?;
            continue;
        }

        if target.exists() {
            let message = format!("目标已存在且不受清单管理：{}", target.display());
            upsert_shared_dir(&mut data, rule, &target, SharedDirStatus::Failed, Some(message.clone()));
            manifest::write_manifest(&iteration_dir, &data)?;
            aborted = true;
            abort_reason = Some(message);
            break;
        }

        match copy::copy_snapshot(Path::new(&rule.source_path), &target) {
            Ok(()) => {
                upsert_shared_dir(&mut data, rule, &target, SharedDirStatus::Copied, None);
            }
            Err(error) => {
                let message = error.message();
                upsert_shared_dir(
                    &mut data,
                    rule,
                    &target,
                    SharedDirStatus::Failed,
                    Some(message.clone()),
                );
                manifest::write_manifest(&iteration_dir, &data)?;
                aborted = true;
                abort_reason = Some(message);
                break;
            }
        }
        manifest::write_manifest(&iteration_dir, &data)?;
    }

    if aborted {
        return Ok(CreateBatchResult {
            iteration: request.iteration.clone(),
            iteration_path,
            shared_directories: data.shared_directories.clone(),
            projects: Vec::new(),
            aborted: true,
            abort_reason,
        });
    }

    // 逐项目串行（判定口径 1、2、4）
    let total = request.projects.len() as u32;
    let mut results: Vec<CreateProjectResult> = Vec::new();
    let mut worktree_cache: HashMap<String, Vec<WorktreeEntry>> = HashMap::new();

    for (offset, item) in request.projects.iter().enumerate() {
        let index = offset as u32 + 1;
        let head_mode = if item.branch.is_some() {
            HeadMode::Branch
        } else {
            HeadMode::Detached
        };
        emit(CreateProgress {
            project_id: item.project_id.clone(),
            index,
            total,
            phase: CreatePhase::Queued,
            message: "排队中".to_string(),
        });

        let Some(project_config) = config.projects.iter().find(|project| project.id == item.project_id) else {
            let message = format!("配置中不存在项目「{}」", item.project_id);
            let predicted = predicted_path(&iteration_dir, &data, &item.project_id);
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, "", &item.base_ref, &predicted, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        };
        let repository = PathBuf::from(&project_config.repository_path);
        let repository_str = project_config.repository_path.clone();

        if !repository.is_dir() {
            let message = format!("源仓库不存在：{repository_str}");
            let predicted = predicted_path(&iteration_dir, &data, &item.project_id);
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, &repository_str, &item.base_ref, &predicted, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        }

        // 基分支归一化（失败也保留归一化尝试结果与 message）
        let normalized = match base_ref::remotes_of(&repository)
            .and_then(|remotes| base_ref::normalize(&remotes, &item.base_ref))
        {
            Ok(value) => value,
            Err(error) => {
                let message = error.message();
                let raw = item.base_ref.trim();
                let fallback = if raw.is_empty() { "origin/master" } else { raw };
                let predicted = predicted_path(&iteration_dir, &data, &item.project_id);
                results.push(record_failure(
                    &mut data, &iteration_dir, item, head_mode, &repository_str, fallback, &predicted, message.clone(),
                )?);
                emit(failed_progress(item, index, total, &message));
                continue;
            }
        };
        let runner = GitRunner::new(&repository);

        // 分支名校验（pure 规则 + `git check-ref-format --branch`）
        if let Some(branch) = item.branch.as_deref() {
            let branch_error = validation::validate_branch_name(branch)
                .err()
                .map(|error| error.message())
                .or_else(|| {
                    git::run(&runner.cmd_check_ref_format_branch(branch))
                        .err()
                        .map(|_| format!("分支名未通过 git check-ref-format --branch：{branch}"))
                });
            if let Some(message) = branch_error {
                let predicted = predicted_path(&iteration_dir, &data, &item.project_id);
                results.push(record_failure(
                    &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &predicted, message.clone(),
                )?);
                emit(failed_progress(item, index, total, &message));
                continue;
            }
        }

        // 去重：projectId + branch（不含基分支）；detached（branch = null）永远新建
        if let Some(branch) = item.branch.as_deref() {
            let duplicate = data.projects.iter().find(|record| {
                record.project_id == item.project_id
                    && record.branch.as_deref() == Some(branch)
                    && record.lifecycle == Lifecycle::Active
            });
            if let Some(record) = duplicate {
                let existing_path = PathBuf::from(&record.worktree_path);
                let entries = worktree_cache
                    .entry(repository_str.clone())
                    .or_insert_with(|| list_worktrees(&repository));
                if existing_path.is_dir()
                    && entries
                        .iter()
                        .any(|entry| platform::paths_equal(&entry.path, &existing_path))
                {
                    results.push(CreateProjectResult {
                        project_id: item.project_id.clone(),
                        worktree_path: record.worktree_path.clone(),
                        status: CreateStatus::AlreadyExists,
                        message: None,
                        vendor_status: record.vendor.as_ref().map(|vendor| vendor.status),
                    });
                    emit(CreateProgress {
                        project_id: item.project_id.clone(),
                        index,
                        total,
                        phase: CreatePhase::Completed,
                        message: format!("已存在有效 worktree：{}", record.worktree_path),
                    });
                    continue;
                }
            }
        }

        // 本地已存在同名分支（且不属于上面的有效重复记录）→ 该项目失败、不建目录
        if let Some(branch) = item.branch.as_deref() {
            if git::run_exists(&runner.cmd_show_ref_verify_branch(branch)) {
                let message = format!("本地已存在同名分支：{branch}");
                let predicted = predicted_path(&iteration_dir, &data, &item.project_id);
                results.push(record_failure(
                    &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &predicted, message.clone(),
                )?);
                emit(failed_progress(item, index, total, &message));
                continue;
            }
        }

        // 目录名顺延：占用集合＝清单记录（含 removed / createFailed）目录名 ∪ 公共目录目标名
        // （磁盘上未受清单管理的同名路径由下面的 exists 检查拦截 → createFailed）
        let occupied = manifest::occupied_names(&data);
        let directory = path_utils::next_directory_name(&occupied, &item.project_id);
        let target = path_utils::join_segments(&iteration_dir, &[&directory])?;
        if target.exists() {
            let message = format!("目录已存在且不受清单管理：{}", target.display());
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &target, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        }

        // fetch → 不可变 Commit
        emit(CreateProgress {
            project_id: item.project_id.clone(),
            index,
            total,
            phase: CreatePhase::Fetching,
            message: format!("git fetch {} {}", normalized.remote, normalized.branch),
        });
        if let Err(error) = git::run(&runner.cmd_fetch(&normalized.remote, &normalized.branch)) {
            let message = error.message();
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &target, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        }
        let commit = match git::run(&runner.cmd_rev_parse_verify_commit("FETCH_HEAD")) {
            Ok(output) => output.trim().to_string(),
            Err(error) => {
                let message = format!("解析 FETCH_HEAD 失败：{}", error.message());
                results.push(record_failure(
                    &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &target, message.clone(),
                )?);
                emit(failed_progress(item, index, total, &message));
                continue;
            }
        };
        if commit.len() != 40 || !commit.chars().all(|ch| ch.is_ascii_hexdigit()) {
            let message = format!("FETCH_HEAD 不是 40 位 Commit：{commit}");
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &target, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        }

        // worktree add
        let add_message = match item.branch.as_deref() {
            Some(branch) => format!("git worktree add -b {branch} {} {commit}", target.display()),
            None => format!("git worktree add --detach {} {commit}", target.display()),
        };
        emit(CreateProgress {
            project_id: item.project_id.clone(),
            index,
            total,
            phase: CreatePhase::Creating,
            message: add_message,
        });
        let add_result = match item.branch.as_deref() {
            Some(branch) => git::run(&runner.cmd_worktree_add_branch(branch, &target, &commit)),
            None => git::run(&runner.cmd_worktree_add_detach(&target, &commit)),
        };
        if let Err(error) = add_result {
            let message = error.message();
            results.push(record_failure(
                &mut data, &iteration_dir, item, head_mode, &repository_str, &normalized.base_ref, &target, message.clone(),
            )?);
            emit(failed_progress(item, index, total, &message));
            continue;
        }

        // vendor（判定口径 4：六条件；不满足是可解释的跳过而非创建失败）
        emit(CreateProgress {
            project_id: item.project_id.clone(),
            index,
            total,
            phase: CreatePhase::Vendor,
            message: "检查 composer.lock 一致性并决定是否复制 vendor".to_string(),
        });
        let (vendor_record, post_step) = handle_vendor(&target, &repository)?;
        let vendor_status = vendor_record.as_ref().map(|record| record.status);

        // 记录写入清单并原子写盘（每项目一次）
        data.projects.push(ManifestProject {
            project_id: item.project_id.clone(),
            source_repository: repository_str.clone(),
            worktree_path: target.to_string_lossy().to_string(),
            head_mode,
            branch: item.branch.clone(),
            base_commit: Some(commit),
            base_ref: normalized.base_ref.clone(),
            created_at: crate::models::now_rfc3339(),
            lifecycle: Lifecycle::Active,
            create_result: CreateResult {
                status: CreateStatus::Created,
                message: None,
            },
            vendor: vendor_record,
            post_steps: vec![post_step],
            removed_at: None,
        });
        manifest::write_manifest(&iteration_dir, &data)?;
        results.push(CreateProjectResult {
            project_id: item.project_id.clone(),
            worktree_path: target.to_string_lossy().to_string(),
            status: CreateStatus::Created,
            message: None,
            vendor_status,
        });
        emit(CreateProgress {
            project_id: item.project_id.clone(),
            index,
            total,
            phase: CreatePhase::Completed,
            message: format!("已完成：{}", target.display()),
        });
    }

    Ok(CreateBatchResult {
        iteration: request.iteration.clone(),
        iteration_path,
        shared_directories: data.shared_directories.clone(),
        projects: results,
        aborted: false,
        abort_reason: None,
    })
}

// ================================== 辅助函数 ==================================

fn upsert_shared_dir(
    data: &mut Manifest,
    rule: &crate::models::SharedDirectoryRule,
    target: &Path,
    status: SharedDirStatus,
    message: Option<String>,
) {
    let record = SharedDirRecord {
        rule_id: rule.target_directory.clone(),
        source_path: rule.source_path.clone(),
        target_directory: rule.target_directory.clone(),
        target_path: target.to_string_lossy().to_string(),
        status,
        message,
    };
    match data
        .shared_directories
        .iter_mut()
        .find(|existing| existing.rule_id == rule.target_directory)
    {
        Some(existing) => *existing = record,
        None => data.shared_directories.push(record),
    }
}

/// 失败记录的预期目录（占用集合＝清单记录 ∪ 公共目录目标名）
fn predicted_path(iteration_dir: &Path, data: &Manifest, project_id: &str) -> PathBuf {
    let occupied = manifest::occupied_names(data);
    iteration_dir.join(path_utils::next_directory_name(&occupied, project_id))
}

fn list_worktrees(repository: &Path) -> Vec<WorktreeEntry> {
    let runner = GitRunner::new(repository);
    git::run(&runner.cmd_worktree_list_porcelain())
        .map(|output| git::parse_worktree_list(&output))
        .unwrap_or_default()
}

fn failed_progress(
    item: &CreateProjectRequest,
    index: u32,
    total: u32,
    message: &str,
) -> CreateProgress {
    CreateProgress {
        project_id: item.project_id.clone(),
        index,
        total,
        phase: CreatePhase::Failed,
        message: message.to_string(),
    }
}

/// 记录 `createFailed`（保留预期 worktreePath、请求参数与 message）并原子写盘
#[allow(clippy::too_many_arguments)]
fn record_failure(
    data: &mut Manifest,
    iteration_dir: &Path,
    item: &CreateProjectRequest,
    head_mode: HeadMode,
    source_repository: &str,
    base_ref_value: &str,
    worktree_path: &Path,
    message: String,
) -> Result<CreateProjectResult, AppError> {
    data.projects.push(ManifestProject {
        project_id: item.project_id.clone(),
        source_repository: source_repository.to_string(),
        worktree_path: worktree_path.to_string_lossy().to_string(),
        head_mode,
        branch: item.branch.clone(),
        base_commit: None,
        base_ref: base_ref_value.to_string(),
        created_at: crate::models::now_rfc3339(),
        lifecycle: Lifecycle::CreateFailed,
        create_result: CreateResult {
            status: CreateStatus::Failed,
            message: Some(message.clone()),
        },
        vendor: None,
        post_steps: Vec::new(),
        removed_at: None,
    });
    manifest::write_manifest(iteration_dir, data)?;
    Ok(CreateProjectResult {
        project_id: item.project_id.clone(),
        worktree_path: worktree_path.to_string_lossy().to_string(),
        status: CreateStatus::Failed,
        message: Some(message),
        vendor_status: None,
    })
}

/// vendor 六条件判定与复制；不满足是可解释的跳过，复制失败保留 worktree
fn handle_vendor(
    worktree: &Path,
    repository: &Path,
) -> Result<(Option<VendorRecord>, PostStepRecord), AppError> {
    let vendor_source = repository.join(vendor::VENDOR_DIR);
    let lock_hash = vendor::lock_hash(&repository.join(vendor::COMPOSER_LOCK)).unwrap_or_default();

    let build = |status: VendorStatus, copied_by_tool: bool, message: Option<String>| VendorRecord {
        source_path: vendor_source.to_string_lossy().to_string(),
        lock_hash: lock_hash.clone(),
        status,
        copied_by_tool,
        message,
    };

    match vendor::assess_vendor(worktree, repository) {
        Ok(vendor::VendorDecision::Copied) => match vendor::copy_vendor(worktree, repository) {
            Ok(()) => Ok((
                Some(build(VendorStatus::Copied, true, None)),
                PostStepRecord {
                    name: "vendor".to_string(),
                    status: PostStepStatus::Success,
                    message: None,
                },
            )),
            Err(error) => {
                let message = error.message();
                Ok((
                    Some(build(VendorStatus::CopyFailed, false, Some(message.clone()))),
                    PostStepRecord {
                        name: "vendor".to_string(),
                        status: PostStepStatus::Failed,
                        message: Some(message),
                    },
                ))
            }
        },
        Ok(decision) => {
            let status = decision_status(decision);
            let message = decision_message(status);
            Ok((
                Some(build(status, false, None)),
                PostStepRecord {
                    name: "vendor".to_string(),
                    status: PostStepStatus::Skipped,
                    message: Some(message),
                },
            ))
        }
        Err(error) => {
            let message = format!("vendor 判定失败：{}", error.message());
            Ok((
                Some(build(VendorStatus::CopyFailed, false, Some(message.clone()))),
                PostStepRecord {
                    name: "vendor".to_string(),
                    status: PostStepStatus::Failed,
                    message: Some(message),
                },
            ))
        }
    }
}

fn decision_status(decision: vendor::VendorDecision) -> VendorStatus {
    match decision {
        vendor::VendorDecision::Copied => VendorStatus::Copied,
        vendor::VendorDecision::NotPhp => VendorStatus::NotPhp,
        vendor::VendorDecision::SourceMissing => VendorStatus::SourceMissing,
        vendor::VendorDecision::LockMissing => VendorStatus::LockMissing,
        vendor::VendorDecision::LockMismatch => VendorStatus::LockMismatch,
        vendor::VendorDecision::TargetExists => VendorStatus::TargetExists,
    }
}

fn decision_message(status: VendorStatus) -> String {
    match status {
        VendorStatus::NotPhp => "非 PHP 项目（缺少 composer.json），无需复制 vendor".to_string(),
        VendorStatus::SourceMissing => "源仓库没有 vendor 目录".to_string(),
        VendorStatus::LockMissing => "缺少 composer.lock（新 worktree 或源仓库）".to_string(),
        VendorStatus::LockMismatch => "两份 composer.lock 的 SHA-256 不一致".to_string(),
        VendorStatus::TargetExists => "目标已存在 vendor 目录".to_string(),
        VendorStatus::Copied => "已复制 vendor".to_string(),
        VendorStatus::CopyFailed => "复制 vendor 失败".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_note_trims_and_validates() {
        assert_eq!(normalize_note("   ").unwrap(), None);
        assert_eq!(normalize_note("  等 QA 回归  ").unwrap(), Some("等 QA 回归".to_string()));
        assert!(normalize_note(&"字".repeat(50)).is_ok());
        assert!(normalize_note(&"字".repeat(51)).is_err());
        assert!(normalize_note("a\nb").is_err());
        assert!(normalize_note("a\rb").is_err());
    }

    #[test]
    fn assign_failure_records_expected_path() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        std::fs::create_dir_all(&iteration_dir).unwrap();
        let mut data = Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: "7.3.0".to_string(),
            created_at: crate::models::now_rfc3339(),
            note: None,
            hidden_at: None,
            archived_at: None,
            shared_directories: Vec::new(),
            projects: Vec::new(),
        };
        let item = CreateProjectRequest {
            project_id: "api3".to_string(),
            branch: Some("feature/x".to_string()),
            base_ref: "origin/master".to_string(),
        };
        let result = record_failure(
            &mut data,
            &iteration_dir,
            &item,
            HeadMode::Branch,
            "/repo/api3",
            "origin/master",
            &iteration_dir.join("api3"),
            "测试失败".to_string(),
        )
        .unwrap();

        assert_eq!(result.status, CreateStatus::Failed);
        assert_eq!(data.projects.len(), 1);
        assert_eq!(data.projects[0].lifecycle, Lifecycle::CreateFailed);
        assert_eq!(data.projects[0].branch.as_deref(), Some("feature/x"));
        assert_eq!(data.projects[0].base_commit, None);
        assert!(data.projects[0].worktree_path.ends_with("7.3.0/api3"));
        // 清单已写盘且可再次解析
        let read = manifest::read_manifest(&iteration_dir);
        assert_eq!(read.health, ManifestHealth::Valid);
        assert_eq!(read.manifest.unwrap().projects.len(), 1);
    }
}

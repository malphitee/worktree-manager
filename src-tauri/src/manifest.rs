//! 迭代清单读写健康度与快扫投影（S2 纯部分：不依赖 Git）。
//! 损坏的清单只报错（`damaged`），不覆盖、不重建、不删除。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner, WorktreeEntry};
use crate::head_state::{self, HeadState, HeadStateInput};
use crate::models::{
    AppConfig, HeadMode, Lifecycle, Manifest, ManifestHealth, ManifestProject, Validity,
    WorkspaceGroup, WorkspaceProject,
};
use crate::path_utils;
use crate::platform;
use crate::{atomic_json, validation};

/// 清单文件名
pub const MANIFEST_FILE_NAME: &str = ".worktree-manager.json";

/// `{迭代目录}/.worktree-manager.json`
pub fn manifest_path(iteration_dir: &Path) -> PathBuf {
    iteration_dir.join(MANIFEST_FILE_NAME)
}

/// 清单读取结果：健康度 + 可解析时的清单 + 不可用原因
#[derive(Debug, Clone)]
pub struct ManifestRead {
    pub health: ManifestHealth,
    pub manifest: Option<Manifest>,
    pub message: Option<String>,
}

/// 读取清单并判定健康度（missing / valid / damaged）
pub fn read_manifest(iteration_dir: &Path) -> ManifestRead {
    let path = manifest_path(iteration_dir);
    if !path.is_file() {
        return ManifestRead {
            health: ManifestHealth::Missing,
            manifest: None,
            message: None,
        };
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            return ManifestRead {
                health: ManifestHealth::Damaged,
                manifest: None,
                message: Some(format!("清单不可读：{error}")),
            }
        }
    };
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            return ManifestRead {
                health: ManifestHealth::Damaged,
                manifest: None,
                message: Some(format!("清单解析失败：{error}")),
            }
        }
    };
    if manifest.schema_version != crate::models::SCHEMA_VERSION {
        return ManifestRead {
            health: ManifestHealth::Damaged,
            manifest: None,
            message: Some(format!(
                "清单 schemaVersion 不是 {}：{}",
                crate::models::SCHEMA_VERSION,
                manifest.schema_version
            )),
        };
    }
    let expected_iteration = directory_name_of(iteration_dir);
    if manifest.iteration != expected_iteration {
        return ManifestRead {
            health: ManifestHealth::Damaged,
            manifest: None,
            message: Some(format!(
                "清单 iteration「{}」与目录名「{expected_iteration}」不一致",
                manifest.iteration
            )),
        };
    }
    if let Some(duplicate) = first_duplicate_worktree_path(&manifest) {
        return ManifestRead {
            health: ManifestHealth::Damaged,
            manifest: None,
            message: Some(format!("清单 worktreePath 重复：{duplicate}")),
        };
    }
    ManifestRead {
        health: ManifestHealth::Valid,
        manifest: Some(manifest),
        message: None,
    }
}

/// 只取健康度（不关心内容）
pub fn health_of(iteration_dir: &Path) -> Result<ManifestHealth, AppError> {
    Ok(read_manifest(iteration_dir).health)
}

/// 原子写清单
pub fn write_manifest(iteration_dir: &Path, manifest: &Manifest) -> Result<(), AppError> {
    atomic_json::write_json_atomic(&manifest_path(iteration_dir), manifest)
}

/// 读取清单（`missing` / `damaged` 直接返回错误，供需要强校验的流程使用）
pub fn load_manifest_strict(iteration_dir: &Path) -> Result<Manifest, AppError> {
    let read = read_manifest(iteration_dir);
    match read.health {
        ManifestHealth::Valid => Ok(read.manifest.expect("valid 清单必然可解析")),
        ManifestHealth::Missing => Err(AppError::NotFound(format!(
            "迭代清单不存在：{}",
            manifest_path(iteration_dir).display()
        ))),
        ManifestHealth::Damaged => Err(AppError::ManifestDamaged(
            read.message.unwrap_or_else(|| "清单损坏".to_string()),
        )),
    }
}

/// 路径的目录名（单层目录名）
pub fn directory_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 清单占用的目录名集合：全部项目记录（含 `removed` / `createFailed`）+ 全部公共目录目标名
pub fn occupied_names(manifest: &Manifest) -> Vec<String> {
    let mut names: Vec<String> = manifest
        .projects
        .iter()
        .map(|project| directory_name_of(Path::new(&project.worktree_path)))
        .filter(|name| !name.is_empty())
        .collect();
    names.extend(
        manifest
            .shared_directories
            .iter()
            .map(|rule| rule.target_directory.clone()),
    );
    names
}

/// 分支展示：`branch` 模式显示分支名；`detached` 显示 `detached @ <短hash>`
pub fn branch_display(head_mode: HeadMode, branch: Option<&str>, commit: Option<&str>) -> String {
    match head_mode {
        HeadMode::Branch => branch.unwrap_or("").to_string(),
        HeadMode::Detached => match commit {
            Some(commit) if !commit.is_empty() => {
                let short: String = commit.chars().take(7).collect();
                format!("detached @ {short}")
            }
            _ => "detached".to_string(),
        },
    }
}

/// 快扫投影：只判断目录存在性 + 读清单（不跑任何 Git）；`dirty` / `hasChanges` 恒为 `null`，
/// `removable` 恒为 `false`（可移除性需要复核后的 validity，前端不自行推导）。
pub fn quick_scan_projects(manifest: &Manifest) -> Vec<WorkspaceProject> {
    manifest.projects.iter().map(quick_scan_project).collect()
}

fn quick_scan_project(project: &ManifestProject) -> WorkspaceProject {
    let exists = Path::new(&project.worktree_path).is_dir();
    let validity = match project.lifecycle {
        Lifecycle::Removed => Validity::Removed,
        Lifecycle::Active | Lifecycle::CreateFailed => {
            if exists {
                Validity::Unknown
            } else {
                Validity::MissingDirectory
            }
        }
    };
    WorkspaceProject {
        project_id: project.project_id.clone(),
        branch_display: branch_display(
            project.head_mode,
            project.branch.as_deref(),
            project.base_commit.as_deref(),
        ),
        base_commit: project.base_commit.clone(),
        base_ref: Some(project.base_ref.clone()),
        source_repository: project.source_repository.clone(),
        worktree_path: project.worktree_path.clone(),
        vendor_status: project.vendor.as_ref().map(|vendor| vendor.status),
        created_at: Some(project.created_at.clone()),
        validity,
        openable: exists,
        removable: false,
        renamed_from: None,
        dirty: None,
        has_changes: None,
        lifecycle: Some(project.lifecycle),
    }
}

fn first_duplicate_worktree_path(manifest: &Manifest) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    for project in &manifest.projects {
        let duplicate = seen.iter().any(|existing| {
            platform::paths_equal(Path::new(existing), Path::new(&project.worktree_path))
        });
        if duplicate {
            return Some(project.worktree_path.clone());
        }
        seen.push(project.worktree_path.clone());
    }
    None
}

/// 校验迭代号安全且与目录名一致（创建流程用）
pub fn validate_iteration_matches_directory(
    iteration: &str,
    iteration_dir: &Path,
) -> Result<(), AppError> {
    validation::validate_iteration(iteration)?;
    let directory = directory_name_of(iteration_dir);
    if directory != iteration {
        return Err(AppError::Validation(format!(
            "迭代号「{iteration}」与目录名「{directory}」不一致"
        )));
    }
    Ok(())
}


// ============================ 列表投影（快扫 / 复核） ============================

/// 列表投影：`reconcile = false` 只做快扫（枚举根目录一级子目录 + 读清单，不跑任何 Git）；
/// `reconcile = true` 追加 `worktree list` / HEAD / `status` 与 discovered 扫描。
/// `archivedAt` 非空的迭代**不进入返回数组**。
pub fn list_groups(config: &AppConfig, reconcile: bool) -> Result<Vec<WorkspaceGroup>, AppError> {
    let Some(root_raw) = config.workspace_root.as_deref() else {
        return Ok(Vec::new());
    };
    let root = Path::new(root_raw);
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let mut names: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(root)
        .map_err(|error| AppError::Io(format!("读取工作区根目录失败 {}：{error}", root.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| AppError::Io(format!("读取目录项失败：{error}")))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || !entry.path().is_dir() {
            continue;
        }
        names.push(name);
    }
    names.sort();

    let mut groups = Vec::new();
    for name in names {
        let raw_dir = root.join(&name);
        let iteration_dir = raw_dir.canonicalize().unwrap_or_else(|_| raw_dir.clone());
        let read = read_manifest(&iteration_dir);
        if let Some(manifest) = &read.manifest {
            if manifest.archived_at.is_some() {
                continue;
            }
        }

        let mut owned = read.manifest.clone();
        let (shared_directories, projects, manifest_message) = match read.health {
            ManifestHealth::Valid => {
                let manifest = owned.as_mut().expect("valid 清单必然可解析");
                let shared = manifest.shared_directories.clone();
                let (projects, write_back_warning) = if reconcile {
                    reconcile_projects(config, &iteration_dir, manifest)?
                } else {
                    (quick_scan_projects(manifest), None)
                };
                (shared, projects, write_back_warning)
            }
            ManifestHealth::Missing => (
                Vec::new(),
                Vec::new(),
                Some(format!("迭代目录下没有清单文件（{}）", MANIFEST_FILE_NAME)),
            ),
            ManifestHealth::Damaged => (Vec::new(), Vec::new(), read.message.clone()),
        };

        groups.push(WorkspaceGroup {
            iteration: name,
            iteration_path: iteration_dir.to_string_lossy().to_string(),
            manifest_health: read.health,
            manifest_message,
            openable: iteration_dir.is_dir(),
            note: owned.as_ref().and_then(|manifest| manifest.note.clone()),
            hidden_at: owned.as_ref().and_then(|manifest| manifest.hidden_at.clone()),
            shared_directories,
            projects,
        });
    }
    Ok(groups)
}

/// 复核投影：对每条 `active` 记录跑 worktree list / HEAD / status，再追加 discovered 扫描。
/// 单条记录的 Git 失败 → 该行 `unknown`，其他行继续（workflows.md §3.2）。
fn reconcile_projects(
    _config: &AppConfig,
    iteration_dir: &Path,
    manifest: &mut Manifest,
) -> Result<(Vec<WorkspaceProject>, Option<String>), AppError> {
    let mut rows: Vec<WorkspaceProject> = Vec::new();
    let mut cache: HashMap<String, Option<Vec<WorktreeEntry>>> = HashMap::new();
    // 011：本地改名的回写收集（索引 → live 名）
    let mut renamed: Vec<(usize, String)> = Vec::new();

    for (record_index, project) in manifest.projects.iter().enumerate() {
        // removed 记录不跑 Git（workflows.md §3.2 第 7 步）
        if project.lifecycle == Lifecycle::Removed {
            rows.push(quick_scan_project(project));
            continue;
        }
        let source_repository = project.source_repository.clone();
        let entries = worktree_entries(&mut cache, Path::new(&source_repository));
        let Some(entries) = entries else {
            rows.push(source_missing_row(project));
            continue;
        };
        let worktree = Path::new(&project.worktree_path);
        let exists = worktree.is_dir();
        if !exists {
            rows.push(missing_directory_row(project));
            continue;
        }
        let entry = entries
            .iter()
            .find(|entry| platform::paths_equal(&entry.path, worktree));
        let Some(entry) = entry else {
            rows.push(not_registered_row(project));
            continue;
        };
        let _ = entry;
        match inspect_worktree(Path::new(&source_repository), project, worktree) {
            Some(row) => {
                // 011：识别到本地改名 → 收集待回写的 (记录下标, live 名)
                if row.renamed_from.is_some() {
                    renamed.push((record_index, row.branch_display.clone()));
                }
                rows.push(row);
            }
            None => rows.push(unknown_row(project)),
        }
    }

    // discovered 扫描：位于该迭代目录下、但清单没有记录的 worktree
    let recorded: Vec<String> = manifest
        .projects
        .iter()
        .map(|project| project.worktree_path.clone())
        .collect();
    let mut discovered_paths: Vec<PathBuf> = Vec::new();
    for (repository, entries) in &cache {
        let Some(entries) = entries else { continue };
        for entry in entries {
            if !path_utils::is_within(iteration_dir, &entry.path) {
                continue;
            }
            if recorded
                .iter()
                .any(|path| platform::paths_equal(Path::new(path), &entry.path))
            {
                continue;
            }
            discovered_paths.push(entry.path.clone());
            rows.push(discovered_row(repository, entry));
        }
    }

    // 迭代目录下含 .git 文件但不属于任何已配置仓库的子目录
    if let Ok(entries) = std::fs::read_dir(iteration_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !path.join(".git").exists() {
                continue;
            }
            if discovered_paths
                .iter()
                .any(|known| platform::paths_equal(known, &path))
            {
                continue;
            }
            if recorded
                .iter()
                .any(|known| platform::paths_equal(Path::new(known), &path))
            {
                continue;
            }
            rows.push(discovered_row_without_repository(&path));
        }
    }

    // 011：把识别到的本地改名原子回写到清单（只改 branch 字段；失败时不影响投影）
    let mut write_back_warning: Option<String> = None;
    if !renamed.is_empty() {
        for (index, live) in &renamed {
            if let Some(project) = manifest.projects.get_mut(*index) {
                project.branch = Some(live.clone());
            }
        }
        if let Err(error) = write_manifest(iteration_dir, manifest) {
            write_back_warning = Some(format!("分支重命名回写失败：{}", error.message()));
        }
    }

    Ok((rows, write_back_warning))
}

/// 按源仓库缓存 `worktree list --porcelain`；`None` ＝ 源仓库缺失或不是 Git 根
fn worktree_entries<'a>(
    cache: &'a mut HashMap<String, Option<Vec<WorktreeEntry>>>,
    repository: &Path,
) -> Option<&'a Vec<WorktreeEntry>> {
    let key = repository.to_string_lossy().to_string();
    if !cache.contains_key(&key) {
        let value = if repository.is_dir() {
            let runner = GitRunner::new(repository);
            git::run(&runner.cmd_worktree_list_porcelain())
                .ok()
                .map(|output| git::parse_worktree_list(&output))
        } else {
            None
        };
        cache.insert(key.clone(), value);
    }
    cache.get(&key).and_then(|value| value.as_ref())
}

fn base_row(project: &ManifestProject, validity: Validity, openable: bool) -> WorkspaceProject {
    WorkspaceProject {
        project_id: project.project_id.clone(),
        branch_display: branch_display(
            project.head_mode,
            project.branch.as_deref(),
            project.base_commit.as_deref(),
        ),
        base_commit: project.base_commit.clone(),
        base_ref: Some(project.base_ref.clone()),
        source_repository: project.source_repository.clone(),
        worktree_path: project.worktree_path.clone(),
        vendor_status: project.vendor.as_ref().map(|vendor| vendor.status),
        created_at: Some(project.created_at.clone()),
        validity,
        openable,
        removable: false,
        renamed_from: None,
        dirty: None,
        has_changes: None,
        lifecycle: Some(project.lifecycle),
    }
}

fn source_missing_row(project: &ManifestProject) -> WorkspaceProject {
    base_row(project, Validity::SourceMissing, false)
}

fn missing_directory_row(project: &ManifestProject) -> WorkspaceProject {
    base_row(project, Validity::MissingDirectory, false)
}

fn not_registered_row(project: &ManifestProject) -> WorkspaceProject {
    base_row(project, Validity::NotRegistered, true)
}

fn unknown_row(project: &ManifestProject) -> WorkspaceProject {
    let openable = Path::new(&project.worktree_path).is_dir();
    base_row(project, Validity::Unknown, openable)
}

/// 读取 worktree 的 HEAD / live 分支 / status 并判定 validity（失败 → `None` → 该行 unknown）。
/// 011：清单分支在源仓库是否仍存在由 `show-ref --verify --quiet` 真实查询，据此区分
/// `headMismatch`（切分支）与 `Renamed`（本地改名）。
fn inspect_worktree(
    repository: &Path,
    project: &ManifestProject,
    worktree: &Path,
) -> Option<WorkspaceProject> {
    let runner = GitRunner::new(worktree);
    let head = git::run_optional(&runner.cmd_rev_parse_verify_head())
        .ok()
        .flatten()?;
    let head = head.trim().to_string();
    let live_branch = git::run_optional(&runner.cmd_symbolic_ref_head())
        .ok()
        .flatten()
        .map(|value| value.trim().to_string());
    let dirty = git::run(&runner.cmd_status_porcelain())
        .ok()
        .map(|output| git::parse_status(&output).is_dirty());
    // 005：复核档的「基准变动」与合并检查用同一函数（解析链见 merge_check::has_changes）
    let has_changes = crate::merge_check::has_changes(worktree, Some(project.base_ref.as_str()));
    let repository_runner = GitRunner::new(repository);
    let manifest_branch_exists = match project.branch.as_deref() {
        Some(branch) => git::run_exists(&repository_runner.cmd_show_ref_verify_branch(branch)),
        None => false,
    };
    let state = head_state::judge(&HeadStateInput {
        head_mode: project.head_mode,
        branch: project.branch.as_deref(),
        live_branch: live_branch.as_deref(),
        manifest_branch_exists,
    });
    let mut renamed_from: Option<String> = None;
    let validity = match &state {
        HeadState::Valid => Validity::Valid,
        HeadState::Renamed { .. } => {
            renamed_from = project.branch.clone();
            Validity::Valid
        }
        HeadState::HeadMismatch => Validity::HeadMismatch,
    };
    let branch_display = match live_branch.clone() {
        Some(name) => name,
        None => detached_display(&head),
    };

    Some(WorkspaceProject {
        project_id: project.project_id.clone(),
        branch_display,
        base_commit: project.base_commit.clone(),
        base_ref: Some(project.base_ref.clone()),
        source_repository: project.source_repository.clone(),
        worktree_path: project.worktree_path.clone(),
        vendor_status: project.vendor.as_ref().map(|vendor| vendor.status),
        created_at: Some(project.created_at.clone()),
        validity,
        openable: true,
        removable: true,
        renamed_from,
        dirty,
        has_changes,
        lifecycle: Some(project.lifecycle),
    })
}

fn discovered_row(repository: &str, entry: &WorktreeEntry) -> WorkspaceProject {
    discovered_base_row(
        &entry.path,
        entry.branch.as_deref(),
        entry.head.as_deref(),
        repository,
    )
}

fn discovered_row_without_repository(path: &Path) -> WorkspaceProject {
    discovered_base_row(path, None, None, "")
}

fn discovered_base_row(
    worktree: &Path,
    branch: Option<&str>,
    head: Option<&str>,
    repository: &str,
) -> WorkspaceProject {
    let runner = GitRunner::new(worktree);
    let live_branch = git::run_optional(&runner.cmd_symbolic_ref_head())
        .ok()
        .flatten()
        .map(|value| value.trim().to_string());
    let head = git::run_optional(&runner.cmd_rev_parse_verify_head())
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .or_else(|| head.map(|value| value.to_string()));
    let dirty = git::run(&runner.cmd_status_porcelain())
        .ok()
        .map(|output| git::parse_status(&output).is_dirty());
    // discovered 行无清单记录：解析链退化为 `origin/master` → `master`（判定口径 6）
    let has_changes = crate::merge_check::has_changes(worktree, None);
    let branch_display = match live_branch.or_else(|| branch.map(|value| value.to_string())) {
        Some(name) => name,
        None => match head.as_deref() {
            Some(value) if !value.is_empty() => detached_display(value),
            _ => "detached".to_string(),
        },
    };

    WorkspaceProject {
        project_id: directory_name_of(worktree),
        branch_display,
        base_commit: None,
        base_ref: None,
        source_repository: repository.to_string(),
        worktree_path: worktree.to_string_lossy().to_string(),
        vendor_status: None,
        created_at: None,
        validity: Validity::Discovered,
        openable: worktree.is_dir(),
        // 无清单记录无法证明 vendor 由工具复制；未匹配到已配置仓库（sourceRepository 为空）时不可移除
        removable: !repository.is_empty(),
        renamed_from: None,
        dirty,
        has_changes,
        lifecycle: None,
    }
}

fn detached_display(head: &str) -> String {
    let short: String = head.chars().take(7).collect();
    format!("detached @ {short}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateResult, CreateStatus, ManifestProject, SharedDirRecord, SharedDirStatus, VendorRecord, VendorStatus};
    use std::fs;

    fn project(worktree_path: &Path, lifecycle: Lifecycle) -> ManifestProject {
        ManifestProject {
            project_id: "api3".to_string(),
            source_repository: "/repo/api3".to_string(),
            worktree_path: worktree_path.to_string_lossy().to_string(),
            head_mode: HeadMode::Branch,
            branch: Some("feature/x".to_string()),
            base_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            base_ref: "origin/master".to_string(),
            created_at: "2026-07-30T10:00:00Z".to_string(),
            lifecycle,
            create_result: CreateResult {
                status: CreateStatus::Created,
                message: None,
            },
            vendor: Some(VendorRecord {
                source_path: "/repo/api3/vendor".to_string(),
                lock_hash: "hash".to_string(),
                status: VendorStatus::Copied,
                copied_by_tool: true,
                message: None,
            }),
            post_steps: Vec::new(),
            removed_at: None,
        }
    }

    fn manifest(iteration_dir: &Path, projects: Vec<ManifestProject>) -> Manifest {
        Manifest {
            schema_version: crate::models::SCHEMA_VERSION,
            iteration: directory_name_of(iteration_dir),
            created_at: "2026-07-30T09:00:00Z".to_string(),
            note: None,
            hidden_at: None,
            archived_at: None,
            shared_directories: vec![SharedDirRecord {
                rule_id: "fd-common".to_string(),
                source_path: "/src/fd-common".to_string(),
                target_directory: "fd-common".to_string(),
                target_path: iteration_dir.join("fd-common").to_string_lossy().to_string(),
                status: SharedDirStatus::Copied,
                message: None,
            }],
            projects,
        }
    }

    #[test]
    fn valid_manifest_with_existing_directory_is_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        let worktree = iteration_dir.join("api3");
        fs::create_dir_all(&worktree).unwrap();
        let data = manifest(&iteration_dir, vec![project(&worktree, Lifecycle::Active)]);
        write_manifest(&iteration_dir, &data).unwrap();

        let read = read_manifest(&iteration_dir);
        assert_eq!(read.health, ManifestHealth::Valid);
        let projects = quick_scan_projects(read.manifest.as_ref().unwrap());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].validity, Validity::Unknown);
        assert!(projects[0].openable);
        assert!(!projects[0].removable);
        assert_eq!(projects[0].dirty, None);
        assert_eq!(projects[0].has_changes, None);
        assert_eq!(projects[0].branch_display, "feature/x");
        assert_eq!(projects[0].vendor_status, Some(VendorStatus::Copied));
        assert_eq!(projects[0].base_ref.as_deref(), Some("origin/master"));
    }

    #[test]
    fn valid_manifest_with_missing_directory_is_missing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        fs::create_dir_all(&iteration_dir).unwrap();
        let worktree = iteration_dir.join("api3");
        let data = manifest(&iteration_dir, vec![project(&worktree, Lifecycle::Active)]);
        write_manifest(&iteration_dir, &data).unwrap();

        let read = read_manifest(&iteration_dir);
        assert_eq!(read.health, ManifestHealth::Valid);
        let projects = quick_scan_projects(read.manifest.as_ref().unwrap());
        assert_eq!(projects[0].validity, Validity::MissingDirectory);
        assert!(!projects[0].openable);
    }

    #[test]
    fn removed_lifecycle_projects_are_marked_removed() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        let worktree = iteration_dir.join("api3");
        fs::create_dir_all(&worktree).unwrap();
        let mut record = project(&worktree, Lifecycle::Removed);
        record.removed_at = Some("2026-07-31T09:00:00Z".to_string());
        let data = manifest(&iteration_dir, vec![record]);
        write_manifest(&iteration_dir, &data).unwrap();

        let projects = quick_scan_projects(read_manifest(&iteration_dir).manifest.as_ref().unwrap());
        assert_eq!(projects[0].validity, Validity::Removed);
        assert_eq!(projects[0].lifecycle, Some(Lifecycle::Removed));
    }

    #[test]
    fn missing_and_damaged_manifests_yield_no_projects() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        fs::create_dir_all(&iteration_dir).unwrap();

        let missing = read_manifest(&iteration_dir);
        assert_eq!(missing.health, ManifestHealth::Missing);
        assert!(missing.manifest.is_none());

        fs::write(manifest_path(&iteration_dir), "{ not json").unwrap();
        let damaged = read_manifest(&iteration_dir);
        assert_eq!(damaged.health, ManifestHealth::Damaged);
        assert!(damaged.manifest.is_none());
        assert!(damaged.message.is_some());
        // 损坏文件不得被改写
        assert_eq!(fs::read_to_string(manifest_path(&iteration_dir)).unwrap(), "{ not json");
    }

    #[test]
    fn damaged_when_schema_or_iteration_or_duplicate_path_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        let worktree = iteration_dir.join("api3");
        fs::create_dir_all(&worktree).unwrap();

        // schemaVersion 不是 1
        let mut data = manifest(&iteration_dir, vec![project(&worktree, Lifecycle::Active)]);
        data.schema_version = 2;
        write_manifest(&iteration_dir, &data).unwrap();
        assert_eq!(read_manifest(&iteration_dir).health, ManifestHealth::Damaged);

        // iteration 与目录名不一致
        let mut data = manifest(&iteration_dir, vec![project(&worktree, Lifecycle::Active)]);
        data.iteration = "7.4.0".to_string();
        write_manifest(&iteration_dir, &data).unwrap();
        assert_eq!(read_manifest(&iteration_dir).health, ManifestHealth::Damaged);

        // worktreePath 重复
        let data = manifest(
            &iteration_dir,
            vec![
                project(&worktree, Lifecycle::Active),
                project(&worktree, Lifecycle::Removed),
            ],
        );
        write_manifest(&iteration_dir, &data).unwrap();
        assert_eq!(read_manifest(&iteration_dir).health, ManifestHealth::Damaged);
    }

    #[test]
    fn occupied_names_include_removed_records_and_shared_directories() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        let worktree = iteration_dir.join("api3");
        let mut failed = project(&worktree, Lifecycle::CreateFailed);
        failed.worktree_path = iteration_dir.join("api3-2").to_string_lossy().to_string();
        let data = manifest(&iteration_dir, vec![project(&worktree, Lifecycle::Removed), failed]);

        let mut names = occupied_names(&data);
        names.sort();
        assert_eq!(names, vec!["api3", "api3-2", "fd-common"]);
    }

    #[test]
    fn detached_branch_display_uses_short_hash() {
        assert_eq!(
            branch_display(HeadMode::Detached, None, Some("0123456789abcdef")),
            "detached @ 0123456"
        );
        assert_eq!(branch_display(HeadMode::Detached, None, None), "detached");
        assert_eq!(branch_display(HeadMode::Branch, Some("feature/x"), None), "feature/x");
    }
}

// ==================== 备注与隐藏（设计 009 / 010，只改单个字段后原子写回） ====================

/// 只改 `note` 字段并原子写回；返回新的 `note`（清单缺失 → `notFound`，损坏 → `manifestDamaged`）
pub fn update_note(iteration_dir: &Path, note: Option<String>) -> Result<Option<String>, AppError> {
    let mut data = load_manifest_strict(iteration_dir)?;
    if data.archived_at.is_some() {
        return Err(AppError::Conflict("该迭代已归档".to_string()));
    }
    data.note = note;
    write_manifest(iteration_dir, &data)?;
    Ok(data.note)
}

/// 写 / 清 `hiddenAt`；返回新的 `hiddenAt`
pub fn set_hidden(iteration_dir: &Path, hidden: bool) -> Result<Option<String>, AppError> {
    let mut data = load_manifest_strict(iteration_dir)?;
    if data.archived_at.is_some() {
        return Err(AppError::Conflict("该迭代已归档".to_string()));
    }
    data.hidden_at = if hidden {
        Some(crate::models::now_rfc3339())
    } else {
        None
    };
    write_manifest(iteration_dir, &data)?;
    Ok(data.hidden_at)
}

#[cfg(test)]
mod note_hidden_tests {
    use super::*;
    use crate::models::{Lifecycle, ManifestProject, SCHEMA_VERSION};

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        std::fs::create_dir_all(&iteration_dir).unwrap();
        let data = Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: "7.3.0".to_string(),
            created_at: "2026-07-30T09:00:00Z".to_string(),
            note: Some("旧备注".to_string()),
            hidden_at: None,
            archived_at: None,
            shared_directories: Vec::new(),
            projects: vec![ManifestProject {
                project_id: "api3".to_string(),
                source_repository: "/repo/api3".to_string(),
                worktree_path: iteration_dir.join("api3").to_string_lossy().to_string(),
                head_mode: HeadMode::Branch,
                branch: Some("feature/x".to_string()),
                base_commit: Some("0".repeat(40)),
                base_ref: "origin/master".to_string(),
                created_at: "2026-07-30T10:00:00Z".to_string(),
                lifecycle: Lifecycle::Active,
                create_result: crate::models::CreateResult {
                    status: crate::models::CreateStatus::Created,
                    message: None,
                },
                vendor: None,
                post_steps: Vec::new(),
                removed_at: None,
            }],
        };
        write_manifest(&iteration_dir, &data).unwrap();
        (temp, iteration_dir)
    }

    #[test]
    fn update_note_only_touches_note_field() {
        let (_temp, iteration_dir) = fixture();
        let before = load_manifest_strict(&iteration_dir).unwrap();

        let updated = update_note(&iteration_dir, Some("新备注".to_string())).unwrap();
        assert_eq!(updated.as_deref(), Some("新备注"));
        let after = load_manifest_strict(&iteration_dir).unwrap();
        assert_eq!(after.note.as_deref(), Some("新备注"));
        // 其他字段不变
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.hidden_at, before.hidden_at);
        assert_eq!(after.shared_directories, before.shared_directories);

        // 空 → 清除
        assert_eq!(update_note(&iteration_dir, None).unwrap(), None);
        assert_eq!(load_manifest_strict(&iteration_dir).unwrap().note, None);

        // 不残留临时文件
        let leftovers: Vec<String> = std::fs::read_dir(&iteration_dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件残留：{leftovers:?}");
    }

    #[test]
    fn update_note_rejects_missing_and_damaged_manifests_without_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        std::fs::create_dir_all(&iteration_dir).unwrap();
        // 缺失
        let error = update_note(&iteration_dir, Some("x".to_string())).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::NotFound);
        // 损坏：报错且不改写
        std::fs::write(manifest_path(&iteration_dir), "{ 坏").unwrap();
        let error = update_note(&iteration_dir, Some("x".to_string())).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::ManifestDamaged);
        assert_eq!(std::fs::read_to_string(manifest_path(&iteration_dir)).unwrap(), "{ 坏");
    }

    #[test]
    fn set_hidden_rejects_missing_and_damaged_manifests() {
        let temp = tempfile::tempdir().unwrap();
        let iteration_dir = temp.path().join("7.3.0");
        std::fs::create_dir_all(&iteration_dir).unwrap();
        let error = set_hidden(&iteration_dir, true).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::NotFound);
        std::fs::write(manifest_path(&iteration_dir), "{ 坏").unwrap();
        let error = set_hidden(&iteration_dir, true).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::ManifestDamaged);
        assert_eq!(std::fs::read_to_string(manifest_path(&iteration_dir)).unwrap(), "{ 坏");
    }

    #[test]
    fn set_hidden_writes_and_clears_timestamp() {
        let (_temp, iteration_dir) = fixture();
        let hidden_at = set_hidden(&iteration_dir, true).unwrap();
        assert!(hidden_at.is_some());
        assert!(hidden_at.as_deref().unwrap().ends_with('Z'), "RFC 3339 UTC：{hidden_at:?}");
        assert_eq!(load_manifest_strict(&iteration_dir).unwrap().hidden_at, hidden_at);

        assert_eq!(set_hidden(&iteration_dir, false).unwrap(), None);
        assert_eq!(load_manifest_strict(&iteration_dir).unwrap().hidden_at, None);
    }

    #[test]
    fn archived_iterations_reject_note_and_hidden_writes() {
        let (_temp, iteration_dir) = fixture();
        let mut data = load_manifest_strict(&iteration_dir).unwrap();
        data.archived_at = Some(crate::models::now_rfc3339());
        write_manifest(&iteration_dir, &data).unwrap();

        for error in [
            update_note(&iteration_dir, Some("x".to_string())).unwrap_err(),
            set_hidden(&iteration_dir, true).unwrap_err(),
        ] {
            assert_eq!(error.code(), crate::models::ErrorCode::Conflict);
        }
    }
}

#[cfg(test)]
mod list_groups_tests {
    use super::*;
    use crate::models::{AppConfig, SCHEMA_VERSION};

    fn write(iteration_dir: &Path, hidden_at: Option<String>, archived_at: Option<String>) {
        let data = Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: directory_name_of(iteration_dir),
            created_at: "2026-07-30T09:00:00Z".to_string(),
            note: None,
            hidden_at,
            archived_at,
            shared_directories: Vec::new(),
            projects: Vec::new(),
        };
        write_manifest(iteration_dir, &data).unwrap();
    }

    #[test]
    fn archived_takes_precedence_over_hidden_and_hidden_is_projected() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        let hidden_dir = workspace.join("7.2.0");
        let archived_dir = workspace.join("7.3.0");
        std::fs::create_dir_all(&hidden_dir).unwrap();
        std::fs::create_dir_all(&archived_dir).unwrap();
        write(&hidden_dir, Some("2026-07-30T10:00:00Z".to_string()), None);
        write(
            &archived_dir,
            Some("2026-07-30T10:00:00Z".to_string()),
            Some("2026-07-31T10:00:00Z".to_string()),
        );

        let config = AppConfig {
            workspace_root: Some(workspace.to_string_lossy().to_string()),
            ..Default::default()
        };
        for reconcile in [false, true] {
            let groups = list_groups(&config, reconcile).unwrap();
            assert_eq!(groups.len(), 1, "归档迭代不应出现（reconcile={reconcile}）");
            assert_eq!(groups[0].iteration, "7.2.0");
            assert_eq!(groups[0].hidden_at.as_deref(), Some("2026-07-30T10:00:00Z"));
        }
    }

    #[test]
    fn workspace_root_missing_returns_empty_for_both_tiers() {
        let config = AppConfig::default();
        assert!(list_groups(&config, false).unwrap().is_empty());
        assert!(list_groups(&config, true).unwrap().is_empty());
    }
}

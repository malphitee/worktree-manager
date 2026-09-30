//! 迭代清单读写健康度与快扫投影（S2 纯部分：不依赖 Git）。
//! 损坏的清单只报错（`damaged`），不覆盖、不重建、不删除。

use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::models::{
    HeadMode, Lifecycle, Manifest, ManifestHealth, ManifestProject, Validity, WorkspaceProject,
};
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

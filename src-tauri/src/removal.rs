//! 安全移除（requirements.md 判定口径 8；workflows.md §7）：
//! 实时风险评估 + 确认文本严格校验 + 执行前重新评估；不提供 `--force`、不 stash / commit / push / 删分支。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner};
use crate::models::{
    AppConfig, Lifecycle, Manifest, ManifestHealth, ManifestProject, RemovalAssessment, RemovalRisk,
    RemoveRequest, RiskCode, RiskSeverity,
};
use crate::path_utils;
use crate::platform;
use crate::{manifest, validation};

/// 评估上下文
struct AssessContext<'a> {
    iteration: &'a str,
    worktree_path: &'a str,
    /// 源仓库（discovered 由调用方给出；托管行来自清单记录）
    repository: Option<PathBuf>,
    record: Option<&'a ManifestProject>,
}

/// 托管行评估：读清单 → 找记录 → 风险评估
pub fn assess_managed(
    config: &AppConfig,
    iteration: &str,
    project_id: &str,
    worktree_path: &str,
) -> Result<RemovalAssessment, AppError> {
    validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::resolve_allow_missing(Path::new(root))?;
    let raw_dir = path_utils::join_segments(&root, &[iteration])?;
    let iteration_dir = raw_dir.canonicalize().unwrap_or(raw_dir);

    let read = manifest::read_manifest(&iteration_dir);
    let mut risks: Vec<RemovalRisk> = Vec::new();
    let manifest_record = match (&read.health, &read.manifest) {
        (ManifestHealth::Valid, Some(manifest)) => 
            manifest.projects.iter().find(|project| {
                project.project_id == project_id
                    && platform::paths_equal(Path::new(&project.worktree_path), Path::new(worktree_path))
            }),
        _ => {
            risks.push(RemovalRisk {
                code: RiskCode::ManifestInvalid,
                severity: RiskSeverity::Blocking,
                message: read
                    .message
                    .clone()
                    .unwrap_or_else(|| "迭代清单缺失或损坏".to_string()),
                paths: vec![manifest::manifest_path(&iteration_dir).to_string_lossy().to_string()],
            });
            None
        }
    };

    if let Some(record) = manifest_record {
        if record.lifecycle != Lifecycle::Active {
            risks.push(RemovalRisk {
                code: RiskCode::PathInvalid,
                severity: RiskSeverity::Blocking,
                message: format!("清单记录不是 active（当前 {}）", lifecycle_label(record.lifecycle)),
                paths: vec![record.worktree_path.clone()],
            });
        }
    } else if risks.is_empty() {
        risks.push(RemovalRisk {
            code: RiskCode::PathInvalid,
            severity: RiskSeverity::Blocking,
            message: "清单中找不到该 worktree 记录".to_string(),
            paths: vec![worktree_path.to_string()],
        });
    }

    let context = AssessContext {
        iteration,
        worktree_path,
        repository: manifest_record.map(|record| PathBuf::from(&record.source_repository)),
        record: manifest_record,
    };
    assess_core(&iteration_dir, &context, risks)
}

/// discovered 行评估：`source_path` 为该行的源仓库
pub fn assess_discovered(
    config: &AppConfig,
    iteration: &str,
    worktree_path: &str,
    source_path: &str,
) -> Result<RemovalAssessment, AppError> {
    validation::validate_iteration(iteration)?;
    if source_path.trim().is_empty() {
        return Err(AppError::Validation(
            "该 discovered 行未匹配到已配置仓库，无法移除".to_string(),
        ));
    }
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::resolve_allow_missing(Path::new(root))?;
    let raw_dir = path_utils::join_segments(&root, &[iteration])?;
    let iteration_dir = raw_dir.canonicalize().unwrap_or(raw_dir);

    let context = AssessContext {
        iteration,
        worktree_path,
        repository: Some(PathBuf::from(source_path)),
        record: None,
    };
    assess_core(&iteration_dir, &context, Vec::new())
}

fn assess_core(
    iteration_dir: &Path,
    context: &AssessContext<'_>,
    mut risks: Vec<RemovalRisk>,
) -> Result<RemovalAssessment, AppError> {
    let worktree = Path::new(context.worktree_path);
    let confirmation_text = format!(
        "{}/{}",
        context.iteration,
        manifest::directory_name_of(worktree)
    );

    // 路径存在且在迭代目录内
    let canonical = match worktree.canonicalize() {
        Ok(value) => Some(value),
        Err(_) => {
            risks.push(path_invalid("worktree 目录不存在或无法访问", context.worktree_path));
            None
        }
    };
    if let Some(canonical) = &canonical {
        if !path_utils::is_within(iteration_dir, canonical) {
            risks.push(path_invalid("worktree 路径不在迭代目录内", context.worktree_path));
        }
    }

    // worktree list：注册 / locked / prunable
    if let Some(repository) = &context.repository {
        let runner = GitRunner::new(repository);
        match git::run(&runner.cmd_worktree_list_porcelain()) {
            Ok(output) => {
                let entries = git::parse_worktree_list(&output);
                match entries
                    .iter()
                    .find(|entry| platform::paths_equal(&entry.path, worktree))
                {
                    Some(entry) => {
                        if entry.locked {
                            risks.push(RemovalRisk {
                                code: RiskCode::WorktreeLocked,
                                severity: RiskSeverity::Blocking,
                                message: "worktree 已被锁定（git worktree lock）".to_string(),
                                paths: vec![context.worktree_path.to_string()],
                            });
                        }
                        if entry.prunable {
                            risks.push(RemovalRisk {
                                code: RiskCode::Prunable,
                                severity: RiskSeverity::Blocking,
                                message: "worktree 为 prunable，请先在源仓库处理".to_string(),
                                paths: vec![context.worktree_path.to_string()],
                            });
                        }
                    }
                    None => risks.push(path_invalid(
                        "worktree 未在源仓库 worktree list 中注册",
                        context.worktree_path,
                    )),
                }
            }
            Err(error) => risks.push(RemovalRisk {
                code: RiskCode::PathInvalid,
                severity: RiskSeverity::Blocking,
                message: format!("无法读取源仓库 worktree 列表：{}", error.message()),
                paths: vec![context.worktree_path.to_string()],
            }),
        }
    } else {
        risks.push(path_invalid("清单记录缺少源仓库路径", context.worktree_path));
    }

    // 工作区状态：tracked 修改 / untracked
    let mut vendor_only = false;
    if canonical.is_some() {
        let runner = GitRunner::new(worktree);
        match git::run(&runner.cmd_status_porcelain()) {
            Ok(output) => {
                let summary = git::parse_status(&output);
                if !summary.tracked_changes.is_empty() {
                    risks.push(RemovalRisk {
                        code: RiskCode::TrackedChanges,
                        severity: RiskSeverity::Blocking,
                        message: format!("存在 {} 处已跟踪文件的未提交修改", summary.tracked_changes.len()),
                        paths: summary.tracked_changes.clone(),
                    });
                }
                if !summary.untracked.is_empty() {
                    let copied_by_tool = context
                        .record
                        .and_then(|record| record.vendor.as_ref())
                        .map(|vendor| vendor.copied_by_tool)
                        .unwrap_or(false);
                    let all_vendor = summary
                        .untracked
                        .iter()
                        .all(|path| path.starts_with("vendor/") || path == "vendor");
                    if copied_by_tool && all_vendor && summary.tracked_changes.is_empty() {
                        vendor_only = true;
                        risks.push(RemovalRisk {
                            code: RiskCode::UntrackedFiles,
                            severity: RiskSeverity::Warning,
                            message:
                                "仅有本工具复制的 vendor 未被跟踪；可勾选「先删除 vendor」后移除"
                                    .to_string(),
                            paths: summary.untracked.clone(),
                        });
                    } else {
                        risks.push(RemovalRisk {
                            code: RiskCode::UntrackedFiles,
                            severity: RiskSeverity::Blocking,
                            message: format!("存在 {} 个未跟踪文件 / 目录", summary.untracked.len()),
                            paths: summary.untracked.clone(),
                        });
                    }
                }
            }
            Err(error) => risks.push(RemovalRisk {
                code: RiskCode::PathInvalid,
                severity: RiskSeverity::Blocking,
                message: format!("无法读取工作区状态：{}", error.message()),
                paths: vec![context.worktree_path.to_string()],
            }),
        }

        // 未被任何 refs/remotes 包含的提交（分支模式 → unpushedCommits；detached → detachedCommits）
        let head_mode = context
            .record
            .map(|record| record.head_mode)
            .unwrap_or_else(|| {
                if git::run_optional(&runner.cmd_symbolic_ref_head())
                    .ok()
                    .flatten()
                    .is_some()
                {
                    crate::models::HeadMode::Branch
                } else {
                    crate::models::HeadMode::Detached
                }
            });
        let contained = git::run(&runner.cmd_for_each_ref_contains("HEAD"))
            .ok()
            .map(|output| output.trim().to_string())
            .unwrap_or_default();
        if contained.is_empty() {
            let (code, message) = match head_mode {
                crate::models::HeadMode::Detached => (
                    RiskCode::DetachedCommits,
                    "Detached HEAD 上的提交未被任何远端引用包含",
                ),
                crate::models::HeadMode::Branch => (
                    RiskCode::UnpushedCommits,
                    "存在未被任何远端引用包含的本地提交",
                ),
            };
            risks.push(RemovalRisk {
                code,
                severity: RiskSeverity::Blocking,
                message: message.to_string(),
                paths: vec![context.worktree_path.to_string()],
            });
        }
    }

    let blocking = risks
        .iter()
        .any(|risk| risk.severity == RiskSeverity::Blocking);
    // allowed ＝ 风险为空，或唯一风险是「仅工具复制的 vendor 未跟踪」且可清理
    let allowed = !blocking && (risks.is_empty() || vendor_only);

    Ok(RemovalAssessment {
        iteration: context.iteration.to_string(),
        project_id: context
            .record
            .map(|record| record.project_id.clone())
            .unwrap_or_default(),
        worktree_path: context.worktree_path.to_string(),
        confirmation_text,
        allowed,
        vendor_only_cleanup_available: vendor_only,
        risks,
    })
}

fn path_invalid(message: &str, path: &str) -> RemovalRisk {
    RemovalRisk {
        code: RiskCode::PathInvalid,
        severity: RiskSeverity::Blocking,
        message: message.to_string(),
        paths: vec![path.to_string()],
    }
}

fn lifecycle_label(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Removed => "removed",
        Lifecycle::CreateFailed => "createFailed",
    }
}

// ==================================== 执行 ====================================

struct ManagedLoad {
    iteration_dir: PathBuf,
    data: Manifest,
    record: ManifestProject,
    repository: PathBuf,
}

fn load_managed(
    config: &AppConfig,
    iteration: &str,
    project_id: &str,
    worktree_path: &str,
) -> Result<ManagedLoad, AppError> {
    validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::resolve_allow_missing(Path::new(root))?;
    let raw_dir = path_utils::join_segments(&root, &[iteration])?;
    let iteration_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let read = manifest::read_manifest(&iteration_dir);
    let data = match read.health {
        ManifestHealth::Valid => read.manifest.expect("valid 清单必然可解析"),
        ManifestHealth::Missing => {
            return Err(AppError::NotFound(
                "迭代清单不存在，无法移除托管 worktree".to_string(),
            ))
        }
        ManifestHealth::Damaged => {
            return Err(AppError::ManifestDamaged(
                read.message.unwrap_or_else(|| "清单损坏".to_string()),
            ))
        }
    };
    let record = data
        .projects
        .iter()
        .find(|project| {
            project.project_id == project_id
                && platform::paths_equal(Path::new(&project.worktree_path), Path::new(worktree_path))
        })
        .cloned()
        .ok_or_else(|| AppError::NotFound("清单中找不到该 worktree 记录".to_string()))?;
    let repository = PathBuf::from(&record.source_repository);
    if !repository.is_dir() {
        return Err(AppError::Validation(format!(
            "源仓库不存在：{}",
            record.source_repository
        )));
    }
    Ok(ManagedLoad {
        iteration_dir,
        data,
        record,
        repository,
    })
}

/// 托管行移除：重新评估 → 校验确认文本 → （可选）删除 vendor → `worktree remove`（不带 --force）→ 写回清单
pub fn remove_managed(config: &AppConfig, request: &RemoveRequest) -> Result<(), AppError> {
    let assessment = assess_managed(
        config,
        &request.iteration,
        &request.project_id,
        &request.worktree_path,
    )?;
    if request.confirmation != assessment.confirmation_text {
        return Err(AppError::Validation(format!(
            "确认文本不匹配：请输入 {}",
            assessment.confirmation_text
        )));
    }
    if request.remove_copied_vendor && !assessment.vendor_only_cleanup_available {
        return Err(AppError::Validation(
            "当前不满足「仅工具复制的 vendor 未跟踪」条件，不能删除 vendor".to_string(),
        ));
    }
    if !assessment.allowed {
        return Err(AppError::Conflict(risks_text(&assessment.risks)));
    }

    let loaded = load_managed(
        config,
        &request.iteration,
        &request.project_id,
        &request.worktree_path,
    )?;
    if loaded.record.lifecycle != Lifecycle::Active {
        return Err(AppError::Conflict(format!(
            "清单记录不是 active（当前 {}），无法移除",
            lifecycle_label(loaded.record.lifecycle)
        )));
    }
    let worktree = Path::new(&request.worktree_path);

    if request.remove_copied_vendor {
        let vendor_dir = worktree.join(crate::vendor::VENDOR_DIR);
        path_utils::ensure_within(worktree, &vendor_dir)?;
        fs::remove_dir_all(&vendor_dir).map_err(|error| {
            AppError::Io(format!("删除 vendor 失败 {}：{error}", vendor_dir.display()))
        })?;
        // 删除后再确认工作区干净
        let status = git::run(&GitRunner::new(worktree).cmd_status_porcelain())?;
        if git::parse_status(&status).is_dirty() {
            return Err(AppError::Conflict(
                "删除 vendor 后工作区仍有未跟踪内容，已停止移除".to_string(),
            ));
        }
    }

    git::run(&GitRunner::new(&loaded.repository).cmd_worktree_remove(worktree, false))?;

    let mut data = loaded.data;
    let removed_at = crate::models::now_rfc3339();
    if let Some(record) = data.projects.iter_mut().find(|project| {
        project.project_id == request.project_id
            && platform::paths_equal(
                Path::new(&project.worktree_path),
                Path::new(&request.worktree_path),
            )
    }) {
        record.lifecycle = Lifecycle::Removed;
        record.removed_at = Some(removed_at);
    }
    manifest::write_manifest(&loaded.iteration_dir, &data)?;
    Ok(())
}

/// discovered 行移除：以 `source_path` 为源仓库；不写清单；`removeCopiedVendor` 必须为 false
#[allow(clippy::too_many_arguments)]
pub fn remove_discovered(
    config: &AppConfig,
    iteration: &str,
    worktree_path: &str,
    source_path: &str,
    confirmation: &str,
    remove_copied_vendor: bool,
) -> Result<(), AppError> {
    let assessment = assess_discovered(config, iteration, worktree_path, source_path)?;
    if confirmation != assessment.confirmation_text {
        return Err(AppError::Validation(format!(
            "确认文本不匹配：请输入 {}",
            assessment.confirmation_text
        )));
    }
    if remove_copied_vendor {
        return Err(AppError::Validation(
            "discovered 行没有清单记录，不能证明 vendor 由工具复制，禁止删除 vendor".to_string(),
        ));
    }
    if !assessment.allowed {
        return Err(AppError::Conflict(risks_text(&assessment.risks)));
    }
    let repository = PathBuf::from(source_path);
    if !repository.is_dir() {
        return Err(AppError::Validation(format!(
            "源仓库不存在：{source_path}"
        )));
    }
    git::run(
        &GitRunner::new(&repository).cmd_worktree_remove(Path::new(worktree_path), false),
    )?;
    Ok(())
}

fn risks_text(risks: &[RemovalRisk]) -> String {
    let mut text = String::from("存在阻止移除的风险：");
    for (index, risk) in risks.iter().enumerate() {
        if index > 0 {
            text.push('；');
        }
        text.push_str(&risk.message);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CreateResult, CreateStatus, HeadMode, ManifestProject, RemoveRequest, SCHEMA_VERSION,
    };

    fn fixture() -> (tempfile::TempDir, AppConfig, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let iteration_dir = workspace.join("7.3.0");
        std::fs::create_dir_all(&iteration_dir).unwrap();
        let config = AppConfig {
            workspace_root: Some(workspace.to_string_lossy().to_string()),
            ..Default::default()
        };
        (temp, config, iteration_dir)
    }

    fn write_manifest_with_record(iteration_dir: &Path, worktree: &Path, lifecycle: Lifecycle) {
        let data = Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: "7.3.0".to_string(),
            created_at: crate::models::now_rfc3339(),
            note: None,
            hidden_at: None,
            archived_at: None,
            shared_directories: Vec::new(),
            projects: vec![ManifestProject {
                project_id: "api3".to_string(),
                source_repository: "/repo/api3".to_string(),
                worktree_path: worktree.to_string_lossy().to_string(),
                head_mode: HeadMode::Branch,
                branch: Some("feature/x".to_string()),
                base_commit: Some("0".repeat(40)),
                base_ref: "origin/master".to_string(),
                created_at: crate::models::now_rfc3339(),
                lifecycle,
                create_result: CreateResult {
                    status: CreateStatus::Created,
                    message: None,
                },
                vendor: None,
                post_steps: Vec::new(),
                removed_at: None,
            }],
        };
        manifest::write_manifest(iteration_dir, &data).unwrap();
    }

    #[test]
    fn assess_reports_path_invalid_for_missing_directory_and_blocks() {
        let (_temp, config, iteration_dir) = fixture();
        let worktree = iteration_dir.join("api3");
        write_manifest_with_record(&iteration_dir, &worktree, Lifecycle::Active);

        let assessment = assess_managed(&config, "7.3.0", "api3", &worktree.to_string_lossy()).unwrap();
        assert!(!assessment.allowed);
        assert!(assessment
            .risks
            .iter()
            .any(|risk| risk.code == RiskCode::PathInvalid && risk.severity == RiskSeverity::Blocking));
        assert_eq!(assessment.confirmation_text, "7.3.0/api3");
    }

    #[test]
    fn assess_reports_manifest_invalid_when_manifest_damaged() {
        let (_temp, config, iteration_dir) = fixture();
        std::fs::write(manifest::manifest_path(&iteration_dir), "{ 损坏").unwrap();
        let worktree = iteration_dir.join("api3");

        let assessment = assess_managed(&config, "7.3.0", "api3", &worktree.to_string_lossy()).unwrap();
        assert!(!assessment.allowed);
        assert!(assessment
            .risks
            .iter()
            .any(|risk| risk.code == RiskCode::ManifestInvalid));
        // 损坏文件不得被改写
        assert_eq!(
            std::fs::read_to_string(manifest::manifest_path(&iteration_dir)).unwrap(),
            "{ 损坏"
        );
    }

    #[test]
    fn remove_rejects_wrong_confirmation_and_forged_vendor_flag() {
        let (_temp, config, iteration_dir) = fixture();
        let worktree = iteration_dir.join("api3");
        write_manifest_with_record(&iteration_dir, &worktree, Lifecycle::Active);
        let worktree_path = worktree.to_string_lossy().to_string();

        let wrong = RemoveRequest {
            iteration: "7.3.0".to_string(),
            project_id: "api3".to_string(),
            worktree_path: worktree_path.clone(),
            confirmation: "7.3.0/API3".to_string(),
            remove_copied_vendor: false,
        };
        let error = remove_managed(&config, &wrong).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Validation);

        let forged = RemoveRequest {
            confirmation: "7.3.0/api3".to_string(),
            remove_copied_vendor: true,
            ..wrong
        };
        let error = remove_managed(&config, &forged).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Validation);
    }

    #[test]
    fn remove_conflicts_when_risks_block() {
        let (_temp, config, iteration_dir) = fixture();
        let worktree = iteration_dir.join("api3");
        write_manifest_with_record(&iteration_dir, &worktree, Lifecycle::Active);

        let request = RemoveRequest {
            iteration: "7.3.0".to_string(),
            project_id: "api3".to_string(),
            worktree_path: worktree.to_string_lossy().to_string(),
            confirmation: "7.3.0/api3".to_string(),
            remove_copied_vendor: false,
        };
        let error = remove_managed(&config, &request).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Conflict);
        // 清单未被改动
        let read = manifest::read_manifest(&iteration_dir);
        assert_eq!(read.manifest.unwrap().projects[0].lifecycle, Lifecycle::Active);
    }
}

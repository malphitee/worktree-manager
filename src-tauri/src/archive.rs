//! 迭代归档（设计 008）：评估（复用合并检查）→ 批量移除 → 全部成功才写 `archivedAt`。
//! 归档不是删除：保留本地分支、迭代目录、公共目录快照与清单历史。

use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner};
use crate::models::{
    AppConfig, ArchiveAssessment, ArchiveFailure, ArchiveOutcome, ArchiveRecordAssessment,
    ArchiveRequest, ManifestHealth, MergeCellResult, MergeCellStatus, MergeCheckProgress,
    MergeRecordResult,
};
use crate::{manifest, merge_check, path_utils};

/// 评估：复用 `merge_check::check`，事件改发 `archive-progress`（`record` 恒为 `null`）
pub fn assess<F>(config: &AppConfig, iteration: &str, emit: F) -> Result<ArchiveAssessment, AppError>
where
    F: Fn(MergeCheckProgress),
{
    assess_with(config, iteration, &emit)
}

/// 内部版本：`archive()` 需要同时复用同一个 `emit`（评估阶段 + 逐条移除阶段）
fn assess_with<F>(config: &AppConfig, iteration: &str, emit: &F) -> Result<ArchiveAssessment, AppError>
where
    F: Fn(MergeCheckProgress),
{
    let iteration_dir = iteration_directory(config, iteration)?;
    ensure_archivable(&iteration_dir)?;

    let result = merge_check::check(
        config,
        iteration,
        merge_check::MergeFilter::default(),
        |progress| {
            // 归档评估只关心进度文案（设计 008 决策 2）
            emit(MergeCheckProgress {
                record: None,
                ..progress
            });
        },
    )?;

    let records: Vec<ArchiveRecordAssessment> = result
        .records
        .iter()
        .map(|record| {
            let (clean, blockers) = judge_record(record);
            ArchiveRecordAssessment {
                project_id: record.project_id.clone(),
                branch_display: record.branch_display.clone(),
                worktree_path: record.worktree_path.clone(),
                develop: record.develop.clone(),
                master: record.master.clone(),
                dirty: record.dirty.unwrap_or(true),
                clean,
                blockers,
            }
        })
        .collect();
    let clean = records.iter().all(|record| record.clean);

    Ok(ArchiveAssessment {
        iteration: iteration.to_string(),
        confirmation_text: iteration.to_string(),
        checked_at: crate::models::now_rfc3339(),
        clean,
        records,
    })
}

/// `clean` 四条件（设计 008 决策 3）：develop / master ∈ {merged, contained, targetMissing} 且
/// 无未提交改动、本次 fetch 未失败；`blockers` 文案由后端生成。
pub fn judge_record(record: &MergeRecordResult) -> (bool, Vec<String>) {
    let mut blockers: Vec<String> = Vec::new();
    for (label, cell) in [("develop", &record.develop), ("master", &record.master)] {
        blockers.extend(status_blocker(label, cell));
    }
    if record.develop.stale || record.master.stale {
        blockers.push("fetch 失败，结果可能过时".to_string());
    }
    match record.dirty {
        Some(true) => blockers.push("存在未提交改动".to_string()),
        Some(false) => {}
        None => blockers.push("无法读取工作区状态".to_string()),
    }
    (blockers.is_empty(), blockers)
}

fn status_blocker(label: &str, cell: &MergeCellResult) -> Option<String> {
    match cell.status {
        MergeCellStatus::Merged | MergeCellStatus::Contained | MergeCellStatus::TargetMissing => None,
        MergeCellStatus::Unmerged => Some(format!("{label}：未合并")),
        MergeCellStatus::BranchMissing => Some(format!("{label}：分支不存在")),
        MergeCellStatus::NotCheckable => {
            Some(format!("{label}：无法检查（worktree 目录缺失或未注册）"))
        }
        MergeCellStatus::Error => Some(format!(
            "{label}：检查出错{}",
            cell.error_message
                .as_deref()
                .map(|message| format!("（{message}）"))
                .unwrap_or_default()
        )),
    }
}

/// 执行归档：确认文本严格相等 → 重新评估 → 逐条移除 → 全部成功才写 `archivedAt`
pub fn archive<F>(
    config: &AppConfig,
    request: &ArchiveRequest,
    emit: F,
) -> Result<ArchiveOutcome, AppError>
where
    F: Fn(MergeCheckProgress),
{
    if request.confirmation != request.iteration {
        return Err(AppError::Validation(format!(
            "确认文本不匹配：请输入 {}",
            request.iteration
        )));
    }
    let iteration_dir = iteration_directory(config, &request.iteration)?;
    let mut data = ensure_archivable(&iteration_dir)?;

    // 不信任前端传来的 clean：内部重新评估（含 fetch）
    let assessment = assess_with(config, &request.iteration, &emit)?;
    if !assessment.clean && !request.force {
        return Err(AppError::Conflict(
            "存在未满足归档条件的记录（可勾选「强制归档」后重试）".to_string(),
        ));
    }

    let active_ids: Vec<String> = data
        .projects
        .iter()
        .filter(|record| record.lifecycle == crate::models::Lifecycle::Active)
        .map(|record| record.worktree_path.clone())
        .collect();

    let mut removed_count = 0_u32;
    let mut failures: Vec<ArchiveFailure> = Vec::new();

    let message_total = active_ids.len() as u32;
    for (offset, worktree_path) in active_ids.iter().enumerate() {
        let clean = assessment
            .records
            .iter()
            .find(|record| record.worktree_path == *worktree_path)
            .map(|record| record.clean)
            .unwrap_or(false);
        let force = request.force && !clean;
        let record = data
            .projects
            .iter()
            .find(|record| record.worktree_path == *worktree_path)
            .cloned()
            .expect("active 记录必然存在");
        let repository = PathBuf::from(&record.source_repository);
        // 进度事件的 message 写实际执行的命令（008 §3.3）
        emit(MergeCheckProgress {
            iteration: request.iteration.clone(),
            project_id: record.project_id.clone(),
            worktree_path: worktree_path.clone(),
            index: offset as u32 + 1,
            total: message_total,
            phase: crate::models::MergeCheckPhase::Checking,
            message: if force {
                format!("git worktree remove --force {worktree_path}")
            } else {
                format!("git worktree remove {worktree_path}")
            },
            record: None,
        });

        if !repository.is_dir() {
            failures.push(ArchiveFailure {
                project_id: record.project_id.clone(),
                worktree_path: worktree_path.clone(),
                message: format!("源仓库不存在：{}", record.source_repository),
            });
            continue;
        }
        let runner = GitRunner::new(&repository);
        match git::run(&runner.cmd_worktree_remove(Path::new(worktree_path), force)) {
            Ok(_) => {
                if let Some(target) = data
                    .projects
                    .iter_mut()
                    .find(|item| item.worktree_path == *worktree_path)
                {
                    target.lifecycle = crate::models::Lifecycle::Removed;
                    target.removed_at = Some(crate::models::now_rfc3339());
                }
                manifest::write_manifest(&iteration_dir, &data)?;
                removed_count += 1;
            }
            Err(error) => failures.push(ArchiveFailure {
                project_id: record.project_id.clone(),
                worktree_path: worktree_path.clone(),
                message: error.message(),
            }),
        }
    }

    let archived = failures.is_empty();
    if archived {
        data.archived_at = Some(crate::models::now_rfc3339());
        manifest::write_manifest(&iteration_dir, &data)?;
    }

    Ok(ArchiveOutcome {
        iteration: request.iteration.clone(),
        removed_count,
        failed: failures,
        archived,
    })
}

fn iteration_directory(config: &AppConfig, iteration: &str) -> Result<PathBuf, AppError> {
    crate::validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::resolve_allow_missing(Path::new(root))?;
    let raw = path_utils::join_segments(&root, &[iteration])?;
    Ok(raw.canonicalize().unwrap_or(raw))
}

/// 读清单并要求可归档（`valid` 且未归档）
fn ensure_archivable(iteration_dir: &Path) -> Result<crate::models::Manifest, AppError> {
    let read = manifest::read_manifest(iteration_dir);
    match read.health {
        ManifestHealth::Valid => {
            let data = read.manifest.expect("valid 清单必然可解析");
            if data.archived_at.is_some() {
                return Err(AppError::Conflict("该迭代已归档".to_string()));
            }
            Ok(data)
        }
        ManifestHealth::Missing => Err(AppError::NotFound("迭代清单不存在".to_string())),
        ManifestHealth::Damaged => Err(AppError::ManifestDamaged(
            read.message.unwrap_or_else(|| "清单损坏".to_string()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Lifecycle, MergeCellResult};

    fn cell(status: MergeCellStatus, stale: bool) -> MergeCellResult {
        MergeCellResult {
            status,
            unmerged_commits: Vec::new(),
            error_message: None,
            stale,
            dirty: false,
        }
    }

    fn record(develop: MergeCellResult, master: MergeCellResult, dirty: Option<bool>) -> MergeRecordResult {
        MergeRecordResult {
            project_id: "api3".to_string(),
            branch_display: "feature/x".to_string(),
            worktree_path: "/ws/7.3.0/api3".to_string(),
            lifecycle: Lifecycle::Active,
            develop,
            master,
            has_changes: Some(false),
            dirty,
        }
    }

    #[test]
    fn clean_requires_all_four_conditions() {
        // merged / merged / 不脏 / 不 stale → clean
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Merged, false),
            cell(MergeCellStatus::Merged, false),
            Some(false),
        ));
        assert!(clean);
        assert!(blockers.is_empty());

        // contained / targetMissing → clean
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Contained, false),
            cell(MergeCellStatus::TargetMissing, false),
            Some(false),
        ));
        assert!(clean, "blockers：{blockers:?}");

        // unmerged → blocker 文案
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Unmerged, false),
            cell(MergeCellStatus::Merged, false),
            Some(false),
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("develop：未合并")));

        // dirty → blocker
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Merged, false),
            cell(MergeCellStatus::Merged, false),
            Some(true),
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("存在未提交改动")));

        // stale → blocker
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Merged, true),
            cell(MergeCellStatus::Merged, true),
            Some(false),
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("fetch 失败")));
    }

    #[test]
    fn branch_missing_not_checkable_and_error_produce_blockers() {
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::BranchMissing, false),
            cell(MergeCellStatus::NotCheckable, false),
            Some(false),
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("分支不存在")));
        assert!(blockers.iter().any(|blocker| blocker.contains("无法检查")));

        let mut error_cell = cell(MergeCellStatus::Error, false);
        error_cell.error_message = Some("git queue 失败".to_string());
        let (clean, blockers) = judge_record(&record(
            error_cell,
            cell(MergeCellStatus::Merged, false),
            Some(false),
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("检查出错")));

        // dirty 读取失败也阻塞
        let (clean, blockers) = judge_record(&record(
            cell(MergeCellStatus::Merged, false),
            cell(MergeCellStatus::Merged, false),
            None,
        ));
        assert!(!clean);
        assert!(blockers.iter().any(|blocker| blocker.contains("无法读取工作区状态")));
    }
}

//! 合并状态检查（设计 001 / 003 / 005 / 006 / 007）：
//! 每仓库一次 `fetch --no-tags origin develop master` → 逐条三层判定 → 逐条事件推送；
//! 结果为内存快照（带 `checkedAt`），不落盘；只查 `lifecycle = active`。

use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::{self, GitRunner};
use crate::head_state::{self, HeadState, HeadStateInput};
use crate::models::{
    AppConfig, HeadMode, Lifecycle, ManifestHealth, ManifestProject, MergeCellResult,
    MergeCellStatus, MergeCheckPhase, MergeCheckProgress, MergeCheckResult, MergeRecordResult,
};
use crate::{manifest, path_utils, platform};

/// 目标分支固定，不配置化
pub const TARGETS: [&str; 2] = ["develop", "master"];

/// 过滤器：`projectId` / `worktreePath` 均可选（设计 003 决策 6）
#[derive(Debug, Clone, Default)]
pub struct MergeFilter {
    pub project_id: Option<String>,
    pub worktree_path: Option<String>,
}

impl MergeFilter {
    fn matches(&self, record: &ManifestProject) -> bool {
        if let Some(project_id) = &self.project_id {
            if &record.project_id != project_id {
                return false;
            }
        }
        if let Some(worktree_path) = &self.worktree_path {
            if !platform::paths_equal(Path::new(&record.worktree_path), Path::new(worktree_path)) {
                return false;
            }
        }
        true
    }
}

/// 判定对象：live 分支名或 HEAD commit
#[derive(Debug, Clone)]
enum Subject {
    Branch(String),
    Commit(String),
}

impl Subject {
    fn reference(&self) -> &str {
        match self {
            Subject::Branch(name) => name,
            Subject::Commit(hash) => hash,
        }
    }
}

/// 条目级检查上下文（解析失败 → `not_checkable`）
#[derive(Debug, Clone)]
struct RecordContext {
    subject: Option<Subject>,
    branch_display: String,
    dirty: Option<bool>,
    not_checkable: bool,
    branch_missing: bool,
}

/// 入口：对迭代内 `active` 记录逐条判定，逐条通过 `emit` 推送（设计 003）
pub fn check<F>(
    config: &AppConfig,
    iteration: &str,
    filter: MergeFilter,
    emit: F,
) -> Result<MergeCheckResult, AppError>
where
    F: Fn(MergeCheckProgress),
{
    let iteration_dir = iteration_directory(config, iteration)?;
    let read = manifest::read_manifest(&iteration_dir);
    let data = match read.health {
        ManifestHealth::Valid => read.manifest.expect("valid 清单必然可解析"),
        ManifestHealth::Missing => {
            return Err(AppError::NotFound("迭代清单不存在".to_string()))
        }
        ManifestHealth::Damaged => {
            return Err(AppError::ManifestDamaged(
                read.message.unwrap_or_else(|| "清单损坏".to_string()),
            ))
        }
    };

    let records: Vec<ManifestProject> = data
        .projects
        .iter()
        .filter(|record| record.lifecycle == Lifecycle::Active)
        .filter(|record| filter.matches(record))
        .cloned()
        .collect();
    let total = records.len() as u32;

    // 按源仓库分组，保持记录顺序（仓库间串行）
    let mut groups: Vec<(String, Vec<ManifestProject>)> = Vec::new();
    for record in records {
        match groups
            .iter_mut()
            .find(|(repository, _)| repository == &record.source_repository)
        {
            Some((_, list)) => list.push(record),
            None => groups.push((record.source_repository.clone(), vec![record])),
        }
    }

    let mut results: Vec<MergeRecordResult> = Vec::new();
    let mut index: u32 = 0;

    for (repository, group) in groups {
        let repository_path = PathBuf::from(&repository);
        let first = group.first().expect("分组必然非空");
        emit(MergeCheckProgress {
            iteration: iteration.to_string(),
            project_id: first.project_id.clone(),
            worktree_path: first.worktree_path.clone(),
            index,
            total,
            phase: MergeCheckPhase::Fetching,
            message: format!("git fetch --no-tags origin {}", TARGETS.join(" ")),
            record: None,
        });

        // 仓库级：一次 fetch（失败 → stale），目标分支存在性
        let mut stale = true;
        let mut targets_present = vec![false; TARGETS.len()];
        if repository_path.is_dir() {
            let runner = GitRunner::new(&repository_path);
            let targets: Vec<&str> = TARGETS.to_vec();
            stale = git::run(&runner.cmd_fetch_targets("origin", &targets)).is_err();
            let refs = git::run(&runner.cmd_for_each_ref_remotes()).unwrap_or_default();
            for (position, target) in TARGETS.iter().enumerate() {
                let expected = format!("refs/remotes/origin/{target}");
                targets_present[position] = refs.lines().any(|line| line.trim() == expected);
            }
        }

        for record in &group {
            index += 1;
            let context = inspect_record(&repository_path, record);
            let subject_label = match &context.subject {
                Some(subject) => subject.reference().to_string(),
                None => record.project_id.clone(),
            };
            emit(MergeCheckProgress {
                iteration: iteration.to_string(),
                project_id: record.project_id.clone(),
                worktree_path: record.worktree_path.clone(),
                index,
                total,
                phase: MergeCheckPhase::Checking,
                message: format!("git merge-base --is-ancestor {subject_label} origin/develop"),
                record: None,
            });

            let dirty = context.dirty.unwrap_or(false);
            let (develop, master) = if context.not_checkable {
                (
                    cell(MergeCellStatus::NotCheckable, stale, dirty, None),
                    cell(MergeCellStatus::NotCheckable, stale, dirty, None),
                )
            } else if context.branch_missing {
                (
                    cell(MergeCellStatus::BranchMissing, stale, dirty, None),
                    cell(MergeCellStatus::BranchMissing, stale, dirty, None),
                )
            } else if let Some(subject) = &context.subject {
                (
                    judge_target(&repository_path, subject, TARGETS[0], targets_present[0], stale, dirty),
                    judge_target(&repository_path, subject, TARGETS[1], targets_present[1], stale, dirty),
                )
            } else {
                (
                    cell(MergeCellStatus::NotCheckable, stale, dirty, None),
                    cell(MergeCellStatus::NotCheckable, stale, dirty, None),
                )
            };

            let has_changes = if context.not_checkable {
                None
            } else {
                has_changes(Path::new(&record.worktree_path), Some(record.base_ref.as_str()))
            };

            let result = MergeRecordResult {
                project_id: record.project_id.clone(),
                branch_display: context.branch_display.clone(),
                worktree_path: record.worktree_path.clone(),
                lifecycle: record.lifecycle,
                develop,
                master,
                has_changes,
                dirty: context.dirty,
            };
            results.push(result.clone());
            emit(MergeCheckProgress {
                iteration: iteration.to_string(),
                project_id: record.project_id.clone(),
                worktree_path: record.worktree_path.clone(),
                index,
                total,
                phase: MergeCheckPhase::Record,
                message: "完成".to_string(),
                record: Some(result),
            });
        }
    }

    Ok(MergeCheckResult {
        iteration: iteration.to_string(),
        checked_at: crate::models::now_rfc3339(),
        records: results,
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

// ============================== 条目级检查 ==============================

/// 解析 worktree 的 live 状态、dirty 与判定对象；任一环节失败 → `not_checkable`
fn inspect_record(repository: &Path, record: &ManifestProject) -> RecordContext {
    let not_checkable = |dirty: Option<bool>| RecordContext {
        subject: None,
        branch_display: record.project_id.clone(),
        dirty,
        not_checkable: true,
        branch_missing: false,
    };

    let worktree = Path::new(&record.worktree_path);
    if !repository.is_dir() || !worktree.is_dir() {
        return not_checkable(None);
    }
    let repository_runner = GitRunner::new(repository);
    let entries = git::run(&repository_runner.cmd_worktree_list_porcelain())
        .map(|output| git::parse_worktree_list(&output))
        .unwrap_or_default();
    if !entries
        .iter()
        .any(|entry| platform::paths_equal(&entry.path, worktree))
    {
        return not_checkable(None);
    }

    let worktree_runner = GitRunner::new(worktree);
    let Some(head) = git::run_optional(&worktree_runner.cmd_rev_parse_verify_head())
        .ok()
        .flatten()
    else {
        return not_checkable(None);
    };
    let head = head.trim().to_string();
    let live_branch = git::run_optional(&worktree_runner.cmd_symbolic_ref_head())
        .ok()
        .flatten()
        .map(|value| value.trim().to_string());
    let dirty = git::run(&worktree_runner.cmd_status_porcelain())
        .ok()
        .map(|output| git::parse_status(&output).is_dirty());

    // live 状态判定（011）：清单分支是否仍存在决定 headMismatch / Renamed
    let manifest_branch_exists = match record.branch.as_deref() {
        Some(branch) => git::run_exists(&repository_runner.cmd_show_ref_verify_branch(branch)),
        None => false,
    };
    let state = head_state::judge(&HeadStateInput {
        head_mode: record.head_mode,
        branch: record.branch.as_deref(),
        live_branch: live_branch.as_deref(),
        manifest_branch_exists,
    });

    match record.head_mode {
        HeadMode::Detached => RecordContext {
            subject: Some(Subject::Commit(head.clone())),
            branch_display: detached_display(&head),
            dirty,
            not_checkable: false,
            branch_missing: false,
        },
        HeadMode::Branch => match (&state, live_branch) {
            // 一致 / 本地改名：统一按 live 名判定（设计 001 决策 5；回写清单属 011）
            (HeadState::Valid | HeadState::Renamed { .. }, Some(live))
            | (HeadState::HeadMismatch, Some(live)) => RecordContext {
                subject: Some(Subject::Branch(live.clone())),
                branch_display: live,
                dirty,
                not_checkable: false,
                branch_missing: false,
            },
            // 清单说分支模式，实际 detached → live 分支不存在
            _ => RecordContext {
                subject: None,
                branch_display: detached_display(&head),
                dirty,
                not_checkable: false,
                branch_missing: true,
            },
        },
    }
}

fn detached_display(head: &str) -> String {
    let short: String = head.chars().take(7).collect();
    format!("detached @ {short}")
}

// ============================== 三层判定 ==============================

/// 三层判定（前者命中即止）：① `merge-base --is-ancestor`；② `cherry` 无 `+` 行；
/// ③ `merge-tree --write-tree` 结果树等于目标树 → `contained`；都不通过 → `unmerged`。
fn judge_target(
    repository: &Path,
    subject: &Subject,
    target: &str,
    target_present: bool,
    stale: bool,
    dirty: bool,
) -> MergeCellResult {
    if !target_present {
        return cell(MergeCellStatus::TargetMissing, stale, dirty, None);
    }
    let runner = GitRunner::new(repository);
    let subject_ref = subject.reference();
    let target_ref = format!("origin/{target}");

    // 层 1：merge-base --is-ancestor（0 ＝ 祖先；1 ＝ 不是；其他 ＝ 异常）
    match git::run_exit(&runner.cmd_merge_base_is_ancestor(subject_ref, &target_ref)) {
        Ok((0, _)) => return cell(MergeCellStatus::Merged, stale, dirty, None),
        Ok((1, _)) => {}
        Ok((code, _)) => {
            return cell(
                MergeCellStatus::Error,
                stale,
                dirty,
                Some(format!("git merge-base --is-ancestor 退出码 {code}")),
            )
        }
        Err(error) => return cell(MergeCellStatus::Error, stale, dirty, Some(error.message())),
    }

    // 层 2：cherry 无 `+` 行 → merged
    let cherry_output = match git::run_exit(&runner.cmd_cherry(&target_ref, subject_ref)) {
        Ok((0, stdout)) => stdout,
        Ok((code, _)) => {
            return cell(
                MergeCellStatus::Error,
                stale,
                dirty,
                Some(format!("git cherry 退出码 {code}")),
            )
        }
        Err(error) => return cell(MergeCellStatus::Error, stale, dirty, Some(error.message())),
    };
    let pending = parse_cherry(&cherry_output);
    if pending.is_empty() {
        return cell(MergeCellStatus::Merged, stale, dirty, None);
    }

    // 层 3：merge-tree --write-tree 结果树 == 目标树 → contained；退出码 1（冲突）→ unmerged
    match git::run_exit(&runner.cmd_merge_tree_write_tree(&target_ref, subject_ref)) {
        Ok((0, stdout)) => {
            let merged_tree = stdout.lines().next().unwrap_or_default().trim().to_string();
            let target_tree = git::run_optional(&runner.cmd_rev_parse_tree(&target_ref))
                .ok()
                .flatten()
                .unwrap_or_default();
            if !merged_tree.is_empty() && merged_tree == target_tree.trim() {
                return cell(MergeCellStatus::Contained, stale, dirty, None);
            }
            unmerged_cell(&runner, &pending, stale, dirty, None)
        }
        Ok((1, _)) => unmerged_cell(&runner, &pending, stale, dirty, None),
        Ok((code, _)) => unmerged_cell(
            &runner,
            &pending,
            stale,
            dirty,
            Some(format!("git merge-tree 退出码 {code}")),
        ),
        Err(error) => unmerged_cell(&runner, &pending, stale, dirty, Some(error.message())),
    }
}

/// `unmerged` 单元格：`unmergedCommits` ＝ `cherry` 里 `+` 行对应的 `<短hash> <subject>`
fn unmerged_cell(
    runner: &GitRunner,
    pending: &[String],
    stale: bool,
    dirty: bool,
    error_message: Option<String>,
) -> MergeCellResult {
    let commits: Vec<String> = pending
        .iter()
        .map(|hash| {
            git::run_optional(&runner.cmd_log_summary(hash))
                .ok()
                .flatten()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .unwrap_or_else(|| hash.chars().take(7).collect())
        })
        .collect();
    MergeCellResult {
        status: MergeCellStatus::Unmerged,
        unmerged_commits: commits,
        error_message,
        stale,
        dirty,
    }
}

fn cell(
    status: MergeCellStatus,
    stale: bool,
    dirty: bool,
    error_message: Option<String>,
) -> MergeCellResult {
    MergeCellResult {
        status,
        unmerged_commits: Vec::new(),
        error_message,
        stale,
        dirty,
    }
}

/// 解析 `git cherry` 输出中的 `+` 行（未合并的提交 hash）
pub fn parse_cherry(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix('+') {
                let hash = rest.split_whitespace().next().unwrap_or_default();
                if hash.is_empty() {
                    None
                } else {
                    Some(hash.to_string())
                }
            } else {
                None
            }
        })
        .collect()
}

// ============================ 基准变动（005） ============================

/// `hasChanges` 解析链：`baseRef` → 去 remote 前缀的本地名 → `origin/master` → `master`，
/// 取第一个可解析者；都不可解析 → `None`；`rev-list --count <base>..HEAD > 0` → `true`。
pub fn has_changes(worktree: &Path, base_ref: Option<&str>) -> Option<bool> {
    if !worktree.is_dir() {
        return None;
    }
    let runner = GitRunner::new(worktree);
    let mut candidates: Vec<String> = Vec::new();
    if let Some(base) = base_ref {
        let trimmed = base.trim();
        if !trimmed.is_empty() {
            candidates.push(trimmed.to_string());
            if let Some(slash) = trimmed.find('/') {
                let local = trimmed[slash + 1..].to_string();
                if !local.is_empty() {
                    candidates.push(local);
                }
            }
        }
    }
    candidates.push("origin/master".to_string());
    candidates.push("master".to_string());

    let base = candidates
        .into_iter()
        .find(|candidate| git::run_exists(&runner.cmd_rev_parse_verify_commit(candidate)))?;
    let count = git::run(&runner.cmd_rev_list_count(&format!("{base}..HEAD")))
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cherry_handles_mixed_empty_and_missing_trailing_newline() {
        let mixed = "+ 1111111111111111111111111111111111111111 提交一\n- 2222222222222222222222222222222222222222 已合并\n+ 3333333333333333333333333333333333333333 提交三";
        assert_eq!(
            parse_cherry(mixed),
            vec![
                "1111111111111111111111111111111111111111".to_string(),
                "3333333333333333333333333333333333333333".to_string()
            ]
        );
        assert!(parse_cherry("").is_empty());
        assert!(parse_cherry("- 2222222222222222222222222222222222222222 已合并").is_empty());
        assert_eq!(parse_cherry("+ abc123").len(), 1);
    }

    #[test]
    fn filter_matches_project_and_worktree_path_case_insensitively() {
        let record = ManifestProject {
            project_id: "api3".to_string(),
            source_repository: "/repo/api3".to_string(),
            worktree_path: "/ws/7.3.0/API3".to_string(),
            head_mode: HeadMode::Branch,
            branch: Some("feature/x".to_string()),
            base_commit: None,
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
        };

        assert!(MergeFilter::default().matches(&record));
        assert!(MergeFilter {
            project_id: Some("api3".to_string()),
            worktree_path: None,
        }
        .matches(&record));
        // 路径比较大小写不敏感
        assert!(MergeFilter {
            project_id: None,
            worktree_path: Some("/ws/7.3.0/api3".to_string()),
        }
        .matches(&record));
        assert!(!MergeFilter {
            project_id: Some("other".to_string()),
            worktree_path: None,
        }
        .matches(&record));
        assert!(!MergeFilter {
            project_id: None,
            worktree_path: Some("/ws/7.3.0/nope".to_string()),
        }
        .matches(&record));
    }
}

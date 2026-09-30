//! 基分支归一化（requirements.md 判定口径 12 / 设计 013）。
//! 说明：S3 的创建流程需要归一化结果，因此本模块在 S3 先落地 `normalize`；
//! 远端分支列举（`list_remote_branches`）按计划在 S8 追加。

use std::path::Path;

use crate::error::AppError;
use crate::models::AppConfig;
use crate::git::{self, GitRunner};

/// 归一化结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedBaseRef {
    /// 归一化全文，形如 `origin/release`
    pub base_ref: String,
    pub remote: String,
    /// 远端分支名（可含 `/`）
    pub branch: String,
}

/// 列举仓库 remote（`git remote`）
pub fn remotes_of(repo: &Path) -> Result<Vec<String>, AppError> {
    let runner = GitRunner::new(repo);
    let output = git::run(&runner.cmd_remote())?;
    Ok(output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

/// 归一化：空串＝`origin/master`；`<remote>/<branch>` 首段命中已配置 remote 则原样；
/// 裸名优先 `origin`，没有 `origin` 且只有一个 remote 时用该 remote；否则报错。
pub fn normalize(remotes: &[String], input: &str) -> Result<NormalizedBaseRef, AppError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(NormalizedBaseRef {
            base_ref: "origin/master".to_string(),
            remote: "origin".to_string(),
            branch: "master".to_string(),
        });
    }

    if let Some(slash) = raw.find('/') {
        let head = &raw[..slash];
        let branch = &raw[slash + 1..];
        if remotes.iter().any(|remote| remote.as_str() == head) && !branch.is_empty() {
            validate_branch_shape(branch)?;
            return Ok(NormalizedBaseRef {
                base_ref: raw.to_string(),
                remote: head.to_string(),
                branch: branch.to_string(),
            });
        }
        return Err(AppError::Validation(format!(
            "remote「{head}」不在仓库 remote 列表中"
        )));
    }

    if remotes.iter().any(|remote| remote == "origin") {
        validate_branch_shape(raw)?;
        return Ok(NormalizedBaseRef {
            base_ref: format!("origin/{raw}"),
            remote: "origin".to_string(),
            branch: raw.to_string(),
        });
    }
    if remotes.len() == 1 {
        validate_branch_shape(raw)?;
        let remote = remotes[0].clone();
        return Ok(NormalizedBaseRef {
            base_ref: format!("{remote}/{raw}"),
            remote,
            branch: raw.to_string(),
        });
    }
    Err(AppError::Validation(
        "仓库没有 origin remote，请填写 <remote>/<branch>".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remotes(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn empty_input_is_origin_master() {
        let normalized = normalize(&remotes(&["origin"]), "").unwrap();
        assert_eq!(normalized.base_ref, "origin/master");
        assert_eq!(normalized.remote, "origin");
        assert_eq!(normalized.branch, "master");
        assert_eq!(normalize(&remotes(&["upstream"]), "   ").unwrap().base_ref, "origin/master");
    }

    #[test]
    fn bare_name_prefers_origin() {
        let normalized = normalize(&remotes(&["upstream", "origin"]), "release").unwrap();
        assert_eq!(normalized.base_ref, "origin/release");
        assert_eq!(normalized.branch, "release");
    }

    #[test]
    fn bare_name_uses_single_remote_without_origin() {
        let normalized = normalize(&remotes(&["upstream"]), "develop").unwrap();
        assert_eq!(normalized.base_ref, "upstream/develop");
        assert_eq!(normalized.remote, "upstream");
    }

    #[test]
    fn explicit_remote_prefix_is_kept() {
        let normalized = normalize(&remotes(&["origin", "upstream"]), "upstream/release/1.2").unwrap();
        assert_eq!(normalized.base_ref, "upstream/release/1.2");
        assert_eq!(normalized.remote, "upstream");
        assert_eq!(normalized.branch, "release/1.2");
    }

    #[test]
    fn unknown_remote_and_ambiguous_bare_name_are_rejected() {
        assert!(normalize(&remotes(&["origin"]), "nope/x").is_err());
        assert!(normalize(&remotes(&[]), "master").is_err());
        assert!(normalize(&remotes(&["a", "b"]), "master").is_err());
    }
}

/// 分支名形状校验（本地规则；`git check-ref-format --branch` 由创建流程追加）
fn validate_branch_shape(branch: &str) -> Result<(), AppError> {
    crate::validation::validate_branch_name(branch)
}

/// 列举基分支候选（设计 013）：`include_remote=false` 只读本地 `refs/remotes`；
/// `true` 时对每个 remote 执行 `ls-remote --heads`（15 秒超时），失败降级为本地候选并给 `warning`。
pub fn list_remote_branches(
    config: &AppConfig,
    project_id: &str,
    include_remote: bool,
) -> Result<crate::models::RemoteBranches, AppError> {
    use crate::models::{BranchSource, RemoteBranches};

    let project = config
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| AppError::NotFound(format!("配置中不存在项目「{project_id}」")))?;
    let repository = Path::new(&project.repository_path);
    if !repository.is_dir() {
        return Err(AppError::Validation(format!(
            "源仓库不存在：{}",
            project.repository_path
        )));
    }
    let runner = GitRunner::new(repository);
    let remotes = remotes_of(repository)?;
    let mut branches = local_remote_branches(&runner)?;

    let mut warning: Option<String> = None;
    let mut source = BranchSource::Local;
    if include_remote && !remotes.is_empty() {
        let mut all_ok = true;
        for remote in &remotes {
            match git::run_with_timeout(&runner.cmd_ls_remote_heads(remote), git::LS_REMOTE_TIMEOUT) {
                Ok(output) => {
                    for line in output.lines() {
                        if let Some(name) = line.split('\t').nth(1) {
                            if let Some(short) = name.trim().strip_prefix("refs/heads/") {
                                if !short.is_empty() {
                                    branches.push(format!("{remote}/{short}"));
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    all_ok = false;
                    warning = Some(error.message());
                }
            }
        }
        if all_ok {
            source = BranchSource::Remote;
        }
    }

    branches.sort();
    branches.dedup();
    Ok(RemoteBranches {
        project_id: project_id.to_string(),
        remotes,
        branches,
        source,
        warning,
    })
}

/// 本地 `refs/remotes` 候选（剔除 `refs/remotes/<remote>/HEAD` 这类符号引用）
fn local_remote_branches(runner: &GitRunner) -> Result<Vec<String>, AppError> {
    let output = git::run(&runner.cmd_for_each_ref_remotes())?;
    let mut branches: Vec<String> = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let Some(short) = trimmed.strip_prefix("refs/remotes/") else {
            continue;
        };
        if short.is_empty() || short.ends_with("/HEAD") || short.ends_with("HEAD") {
            continue;
        }
        branches.push(short.to_string());
    }
    Ok(branches)
}

#[cfg(test)]
mod list_tests {
    use super::*;

    #[test]
    fn branch_shape_validation_reuses_local_rules() {
        assert!(validate_branch_shape("feature/x").is_ok());
        assert!(validate_branch_shape("a..b").is_err());
        assert!(validate_branch_shape("").is_err());
    }
}

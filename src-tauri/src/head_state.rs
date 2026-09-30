//! 分支 live 状态判定（设计 011）。
//! S3 只用于复核的 `valid` / `headMismatch` 两态；「本地改名」判定与清单回写在 S7 启用。

use crate::models::HeadMode;

/// 判定结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Valid,
    HeadMismatch,
    /// 清单分支在源仓库已不存在，而 worktree 仍注册在同一路径并检出了另一分支（S7 启用）
    Renamed { live: String },
}

/// 判定输入（避免在纯函数里调用 Git）
#[derive(Debug, Clone)]
pub struct HeadStateInput<'a> {
    pub head_mode: HeadMode,
    /// 清单记录的分支名
    pub branch: Option<&'a str>,
    /// `symbolic-ref -q --short HEAD` 结果；`None` ＝ detached
    pub live_branch: Option<&'a str>,
    /// 清单分支在源仓库是否仍存在（`show-ref --verify --quiet refs/heads/<branch>`）
    pub manifest_branch_exists: bool,
}

/// 判定规则：
/// - `branch` 模式：live 与清单一致 → `Valid`；不一致且清单分支仍存在 → `HeadMismatch`；
///   不一致且清单分支已不存在 → `Renamed { live }`。
/// - `detached` 模式：仍 detached → `Valid`；被检出到某个分支 → `HeadMismatch`。
pub fn judge(input: &HeadStateInput<'_>) -> HeadState {
    match input.head_mode {
        HeadMode::Detached => match input.live_branch {
            None => HeadState::Valid,
            Some(_) => HeadState::HeadMismatch,
        },
        HeadMode::Branch => {
            let Some(branch) = input.branch else {
                // 清单数据异常（branch 模式却无分支名）：按 valid 处理，交由复核其他步骤暴露
                return HeadState::Valid;
            };
            match input.live_branch {
                Some(live) if live == branch => HeadState::Valid,
                Some(live) => {
                    if input.manifest_branch_exists {
                        HeadState::HeadMismatch
                    } else {
                        HeadState::Renamed {
                            live: live.to_string(),
                        }
                    }
                }
                None => HeadState::HeadMismatch,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_mode_same_live_branch_is_valid() {
        let state = judge(&HeadStateInput {
            head_mode: HeadMode::Branch,
            branch: Some("feature/x"),
            live_branch: Some("feature/x"),
            manifest_branch_exists: true,
        });
        assert_eq!(state, HeadState::Valid);
    }

    #[test]
    fn branch_mode_switched_branch_is_head_mismatch() {
        let state = judge(&HeadStateInput {
            head_mode: HeadMode::Branch,
            branch: Some("feature/x"),
            live_branch: Some("feature/y"),
            manifest_branch_exists: true,
        });
        assert_eq!(state, HeadState::HeadMismatch);
    }

    #[test]
    fn branch_mode_missing_manifest_branch_is_renamed() {
        let state = judge(&HeadStateInput {
            head_mode: HeadMode::Branch,
            branch: Some("feature/x"),
            live_branch: Some("feature/renamed"),
            manifest_branch_exists: false,
        });
        assert_eq!(
            state,
            HeadState::Renamed {
                live: "feature/renamed".to_string()
            }
        );
    }

    #[test]
    fn detached_records() {
        assert_eq!(
            judge(&HeadStateInput {
                head_mode: HeadMode::Detached,
                branch: None,
                live_branch: None,
                manifest_branch_exists: false,
            }),
            HeadState::Valid
        );
        assert_eq!(
            judge(&HeadStateInput {
                head_mode: HeadMode::Detached,
                branch: None,
                live_branch: Some("main"),
                manifest_branch_exists: true,
            }),
            HeadState::HeadMismatch
        );
        // 清单说 branch，实际 detached
        assert_eq!(
            judge(&HeadStateInput {
                head_mode: HeadMode::Branch,
                branch: Some("feature/x"),
                live_branch: None,
                manifest_branch_exists: true,
            }),
            HeadState::HeadMismatch
        );
    }
}

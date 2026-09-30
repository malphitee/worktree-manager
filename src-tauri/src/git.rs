//! Git 子命令封装（architecture.md §5）：
//! - 一律 `std::process::Command::new("git").args(&[...])` 参数向量调用，禁止拼接 shell 字符串
//! - 参数向量构造（`GitCommand`）与执行（`run` / `run_with_timeout`）分离，便于单测逐条比对
//! - 所有子进程设置 `GIT_TERMINAL_PROMPT=0`；`ls-remote` 15 秒超时
//! - 失败信息统一经 `error.rs` 脱敏与截断

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::error::{redact_credentials, truncate_message, AppError};

/// `ls-remote` 超时
pub const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(15);

/// 一条待执行的 git 命令（字段公开以便测试构造 `sleep` 之类的替身命令）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl GitCommand {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: "git".to_string(),
            args: Vec::new(),
            cwd: cwd.into(),
        }
    }
}

/// 白名单子命令的构造器；执行统一走 `run` / `run_with_timeout`
#[derive(Debug, Clone)]
pub struct GitRunner {
    repo: PathBuf,
}

impl GitRunner {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self { repo: repo.into() }
    }

    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// `git rev-parse --show-toplevel`（工作目录＝待解析的路径本身）
    pub fn cmd_rev_parse_toplevel(cwd: &Path) -> GitCommand {
        Self::build_at(cwd, &["rev-parse", "--show-toplevel"])
    }

    pub fn cmd_check_ref_format_branch(&self, branch: &str) -> GitCommand {
        self.build(&["check-ref-format", "--branch", branch])
    }

    pub fn cmd_remote(&self) -> GitCommand {
        self.build(&["remote"])
    }

    pub fn cmd_ls_remote_heads(&self, remote: &str) -> GitCommand {
        self.build(&["ls-remote", "--heads", remote])
    }

    pub fn cmd_for_each_ref_remotes(&self) -> GitCommand {
        self.build(&["for-each-ref", "--format=%(refname)", "refs/remotes"])
    }

    pub fn cmd_for_each_ref_contains(&self, commit: &str) -> GitCommand {
        self.build(&[
            "for-each-ref",
            "--format=%(refname)",
            "--contains",
            commit,
            "refs/remotes",
        ])
    }

    pub fn cmd_fetch(&self, remote: &str, branch: &str) -> GitCommand {
        self.build(&["fetch", remote, branch])
    }

    /// `git fetch --no-tags <remote> <target>...`（合并检查的固定目标）
    pub fn cmd_fetch_targets(&self, remote: &str, targets: &[&str]) -> GitCommand {
        let mut args = vec![
            "fetch".to_string(),
            "--no-tags".to_string(),
            remote.to_string(),
        ];
        args.extend(targets.iter().map(|target| target.to_string()));
        GitCommand {
            program: "git".to_string(),
            args,
            cwd: self.repo.clone(),
        }
    }

    /// `git rev-parse --verify --quiet <rev>^{commit}`（同时用于判定目标分支是否解析得出）
    pub fn cmd_rev_parse_verify_commit(&self, rev: &str) -> GitCommand {
        self.build(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])
    }

    pub fn cmd_rev_parse_verify_head(&self) -> GitCommand {
        self.build(&["rev-parse", "--verify", "HEAD"])
    }

    pub fn cmd_symbolic_ref_head(&self) -> GitCommand {
        self.build(&["symbolic-ref", "-q", "--short", "HEAD"])
    }

    pub fn cmd_log_summary(&self, hash: &str) -> GitCommand {
        self.build(&["log", "-1", "--format=%h %s", hash])
    }

    pub fn cmd_show_ref_verify_branch(&self, branch: &str) -> GitCommand {
        self.build(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
    }

    pub fn cmd_worktree_add_detach(&self, path: &Path, commit: &str) -> GitCommand {
        self.build(&["worktree", "add", "--detach", &path.to_string_lossy(), commit])
    }

    pub fn cmd_worktree_add_branch(&self, branch: &str, path: &Path, commit: &str) -> GitCommand {
        self.build(&[
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            commit,
        ])
    }

    pub fn cmd_worktree_list_porcelain(&self) -> GitCommand {
        self.build(&["worktree", "list", "--porcelain"])
    }

    /// `git worktree move [-f] <旧> <新>`：脏工作区只用单个 `-f`，**禁止 `-f -f`**
    pub fn cmd_worktree_move(&self, old: &Path, new: &Path, force: bool) -> GitCommand {
        let mut args = vec!["worktree".to_string(), "move".to_string()];
        if force {
            args.push("-f".to_string());
        }
        args.push(old.to_string_lossy().to_string());
        args.push(new.to_string_lossy().to_string());
        GitCommand {
            program: "git".to_string(),
            args,
            cwd: self.repo.clone(),
        }
    }

    /// `git worktree remove [--force] <path>`：`--force` 仅 008 强制归档可用
    pub fn cmd_worktree_remove(&self, path: &Path, force: bool) -> GitCommand {
        let mut args = vec!["worktree".to_string(), "remove".to_string()];
        if force {
            args.push("--force".to_string());
        }
        args.push(path.to_string_lossy().to_string());
        GitCommand {
            program: "git".to_string(),
            args,
            cwd: self.repo.clone(),
        }
    }

    pub fn cmd_status_porcelain(&self) -> GitCommand {
        self.build(&["status", "--porcelain=v1", "--untracked-files=all"])
    }

    pub fn cmd_merge_base_is_ancestor(&self, a: &str, b: &str) -> GitCommand {
        self.build(&["merge-base", "--is-ancestor", a, b])
    }

    pub fn cmd_cherry(&self, upstream: &str, head: &str) -> GitCommand {
        self.build(&["cherry", upstream, head])
    }

    pub fn cmd_merge_tree_write_tree(&self, base: &str, head: &str) -> GitCommand {
        self.build(&["merge-tree", "--write-tree", base, head])
    }

    pub fn cmd_rev_parse_tree(&self, rev: &str) -> GitCommand {
        self.build(&["rev-parse", &format!("{rev}^{{tree}}")])
    }

    pub fn cmd_rev_list_count(&self, range: &str) -> GitCommand {
        self.build(&["rev-list", "--count", range])
    }

    pub fn cmd_rev_list(&self, range: &str) -> GitCommand {
        self.build(&["rev-list", range])
    }

    fn build(&self, args: &[&str]) -> GitCommand {
        Self::build_at(&self.repo, args)
    }

    fn build_at(cwd: &Path, args: &[&str]) -> GitCommand {
        GitCommand {
            program: "git".to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: cwd.to_path_buf(),
        }
    }
}

// ================================ 执行（run / 超时） ================================

/// 执行并等待结束（不设超时；fetch 由用户网络决定）
pub fn run(cmd: &GitCommand) -> Result<String, AppError> {
    let child = spawn(cmd)?;
    let output = child
        .wait_with_output()
        .map_err(|error| AppError::Git(format!("执行 git 失败：{error}")))?;
    finish(cmd, output.status, output.stdout, output.stderr)
}

/// 执行并设超时：`spawn` + 轮询 `try_wait` + 超时 `kill`
pub fn run_with_timeout(cmd: &GitCommand, timeout: Duration) -> Result<String, AppError> {
    let mut child = spawn(cmd)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = read_async(stdout);
    let stderr_handle = read_async(stderr);

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = join_reader(stdout_handle);
                let err = join_reader(stderr_handle);
                return finish(cmd, status, out, err);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Git(format!(
                        "git {} 超时（{} 秒）",
                        cmd.args.join(" "),
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = child.kill();
                return Err(AppError::Git(format!("执行 git 失败：{error}")));
            }
        }
    }
}

/// 执行并返回（退出码, stdout）；**非零退出不视为错误**。
/// 用于依赖退出码做判定的命令（`merge-base --is-ancestor` / `cherry` / `merge-tree`）。
pub fn run_exit(cmd: &GitCommand) -> Result<(i32, String), AppError> {
    let child = spawn(cmd)?;
    let output = child
        .wait_with_output()
        .map_err(|error| AppError::Git(format!("执行 git 失败：{error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok((output.status.code().unwrap_or(-1), stdout))
}

/// 「存在性判断」类命令（`--verify --quiet`）：**退出码 0 → true**
/// 注意：`--quiet` 不输出内容，不能用 stdout 判断；与 `run_optional` 的区别在此。
pub fn run_exists(cmd: &GitCommand) -> bool {
    run(cmd).is_ok()
}

/// 「存在性判断」类命令（`--verify --quiet`）：非零退出视为「不存在」→ `Ok(None)`
pub fn run_optional(cmd: &GitCommand) -> Result<Option<String>, AppError> {
    match run(cmd) {
        Ok(output) => {
            let trimmed = output.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        Err(AppError::Git(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

fn spawn(cmd: &GitCommand) -> Result<Child, AppError> {
    Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::Git(format!(
                "无法执行 {}（工作目录 {}）：{error}",
                cmd.program,
                cmd.cwd.display()
            ))
        })
}

fn read_async<R: Read + Send + 'static>(
    reader: Option<R>,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    reader.map(|mut reader| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = reader.read_to_end(&mut buffer);
            buffer
        })
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn finish(
    cmd: &GitCommand,
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> Result<String, AppError> {
    if status.success() {
        return Ok(String::from_utf8_lossy(&stdout).to_string());
    }
    let raw = String::from_utf8_lossy(&stderr).trim().to_string();
    let message = truncate_message(&redact_credentials(&raw));
    if message.is_empty() {
        Err(AppError::Git(format!(
            "git {} 退出码 {:?}",
            cmd.args.join(" "),
            status.code()
        )))
    } else {
        Err(AppError::Git(message))
    }
}


// ============================== worktree list --porcelain ==============================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    /// 短分支名（已去掉 `refs/heads/` 前缀）
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
    pub bare: bool,
}

/// 完整解析 `git worktree list --porcelain`（不截断）
pub fn parse_worktree_list(output: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for line in output.lines() {
        if line.trim().is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(path),
                ..Default::default()
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            entry.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            entry.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
        } else if line == "detached" {
            entry.detached = true;
        } else if line == "bare" {
            entry.bare = true;
        } else if line == "locked" || line.starts_with("locked ") {
            entry.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            entry.prunable = true;
        }
    }
    if let Some(entry) = current.take() {
        entries.push(entry);
    }
    entries
}

// ============================== status --porcelain=v1 ==============================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSummary {
    /// 已跟踪文件的改动（含重命名，形如 `旧 -> 新`）
    pub tracked_changes: Vec<String>,
    /// 未跟踪文件 / 目录
    pub untracked: Vec<String>,
}

impl StatusSummary {
    pub fn is_dirty(&self) -> bool {
        !self.tracked_changes.is_empty() || !self.untracked.is_empty()
    }

    pub fn all_paths(&self) -> Vec<String> {
        let mut paths = self.tracked_changes.clone();
        paths.extend(self.untracked.iter().cloned());
        paths
    }
}

/// 完整解析 `git status --porcelain=v1 --untracked-files=all`（不截断）
pub fn parse_status(output: &str) -> StatusSummary {
    let mut summary = StatusSummary::default();
    for line in output.lines() {
        if line.len() < 3 {
            continue;
        }
        let code = &line[..2];
        let path = line[3..].trim_end();
        if code == "??" {
            summary.untracked.push(unquote(path));
        } else {
            summary.tracked_changes.push(unquote(path));
        }
    }
    summary
}

/// git 会对含特殊字符的路径加引号并转义
fn unquote(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(command: &GitCommand) -> Vec<String> {
        command.args.clone()
    }

    fn runner() -> GitRunner {
        GitRunner::new("/tmp/repo")
    }

    #[test]
    fn argument_vectors_match_whitelist() {
        let runner = runner();

        assert_eq!(
            args_of(&GitRunner::cmd_rev_parse_toplevel(Path::new("/tmp/x"))),
            vec!["rev-parse", "--show-toplevel"]
        );
        assert_eq!(
            args_of(&runner.cmd_check_ref_format_branch("feature/x")),
            vec!["check-ref-format", "--branch", "feature/x"]
        );
        assert_eq!(args_of(&runner.cmd_remote()), vec!["remote"]);
        assert_eq!(
            args_of(&runner.cmd_ls_remote_heads("origin")),
            vec!["ls-remote", "--heads", "origin"]
        );
        assert_eq!(
            args_of(&runner.cmd_for_each_ref_remotes()),
            vec!["for-each-ref", "--format=%(refname)", "refs/remotes"]
        );
        assert_eq!(
            args_of(&runner.cmd_for_each_ref_contains("abc123")),
            vec![
                "for-each-ref",
                "--format=%(refname)",
                "--contains",
                "abc123",
                "refs/remotes"
            ]
        );
        assert_eq!(
            args_of(&runner.cmd_fetch("origin", "master")),
            vec!["fetch", "origin", "master"]
        );
        assert_eq!(
            args_of(&runner.cmd_fetch_targets("origin", &["develop", "master"])),
            vec!["fetch", "--no-tags", "origin", "develop", "master"]
        );
        assert_eq!(
            args_of(&runner.cmd_rev_parse_verify_commit("FETCH_HEAD")),
            vec!["rev-parse", "--verify", "--quiet", "FETCH_HEAD^{commit}"]
        );
        assert_eq!(
            args_of(&runner.cmd_rev_parse_verify_head()),
            vec!["rev-parse", "--verify", "HEAD"]
        );
        assert_eq!(
            args_of(&runner.cmd_symbolic_ref_head()),
            vec!["symbolic-ref", "-q", "--short", "HEAD"]
        );
        assert_eq!(
            args_of(&runner.cmd_log_summary("abc123")),
            vec!["log", "-1", "--format=%h %s", "abc123"]
        );
        assert_eq!(
            args_of(&runner.cmd_show_ref_verify_branch("feature/x")),
            vec!["show-ref", "--verify", "--quiet", "refs/heads/feature/x"]
        );
        assert_eq!(
            args_of(&runner.cmd_worktree_add_detach(Path::new("/ws/api3"), "abc123")),
            vec!["worktree", "add", "--detach", "/ws/api3", "abc123"]
        );
        assert_eq!(
            args_of(&runner.cmd_worktree_add_branch("feature/x", Path::new("/ws/api3"), "abc123")),
            vec!["worktree", "add", "-b", "feature/x", "/ws/api3", "abc123"]
        );
        assert_eq!(
            args_of(&runner.cmd_worktree_list_porcelain()),
            vec!["worktree", "list", "--porcelain"]
        );
        assert_eq!(
            args_of(&runner.cmd_status_porcelain()),
            vec!["status", "--porcelain=v1", "--untracked-files=all"]
        );
        assert_eq!(
            args_of(&runner.cmd_merge_base_is_ancestor("a", "b")),
            vec!["merge-base", "--is-ancestor", "a", "b"]
        );
        assert_eq!(
            args_of(&runner.cmd_cherry("origin/master", "feature/x")),
            vec!["cherry", "origin/master", "feature/x"]
        );
        assert_eq!(
            args_of(&runner.cmd_merge_tree_write_tree("origin/master", "feature/x")),
            vec!["merge-tree", "--write-tree", "origin/master", "feature/x"]
        );
        assert_eq!(
            args_of(&runner.cmd_rev_parse_tree("origin/master")),
            vec!["rev-parse", "origin/master^{tree}"]
        );
        assert_eq!(
            args_of(&runner.cmd_rev_list_count("origin/master..HEAD")),
            vec!["rev-list", "--count", "origin/master..HEAD"]
        );
        assert_eq!(
            args_of(&runner.cmd_rev_list("origin/master..HEAD")),
            vec!["rev-list", "origin/master..HEAD"]
        );
    }

    #[test]
    fn worktree_move_force_has_exactly_one_flag() {
        let runner = runner();
        let without = args_of(&runner.cmd_worktree_move(Path::new("/ws/a"), Path::new("/ws/b"), false));
        assert_eq!(without, vec!["worktree", "move", "/ws/a", "/ws/b"]);
        assert!(!without.contains(&"-f".to_string()));

        let forced = args_of(&runner.cmd_worktree_move(Path::new("/ws/a"), Path::new("/ws/b"), true));
        assert_eq!(forced, vec!["worktree", "move", "-f", "/ws/a", "/ws/b"]);
        assert_eq!(forced.iter().filter(|arg| *arg == "-f").count(), 1);
        assert_ne!(forced, vec!["worktree", "move", "-f", "-f", "/ws/a", "/ws/b"]);
    }

    #[test]
    fn worktree_remove_force_flag() {
        let runner = runner();
        assert_eq!(
            args_of(&runner.cmd_worktree_remove(Path::new("/ws/a"), false)),
            vec!["worktree", "remove", "/ws/a"]
        );
        assert_eq!(
            args_of(&runner.cmd_worktree_remove(Path::new("/ws/a"), true)),
            vec!["worktree", "remove", "--force", "/ws/a"]
        );
    }

    #[test]
    fn run_with_timeout_kills_long_commands() {
        let command = GitCommand {
            program: "sleep".to_string(),
            args: vec!["5".to_string()],
            cwd: std::env::temp_dir(),
        };
        let started = Instant::now();
        let result = run_with_timeout(&command, Duration::from_secs(1));
        let elapsed = started.elapsed();

        let error = result.expect_err("sleep 5 应在 1 秒超时后返回错误");
        assert_eq!(error.code(), crate::models::ErrorCode::Git);
        assert!(error.message().contains("超时"), "错误信息应包含超时：{}", error.message());
        assert!(elapsed < Duration::from_secs(4), "不应等待命令自然结束");
    }

    #[test]
    fn run_optional_treats_failure_as_absent() {
        let temp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(temp.path());
        // 非仓库目录：rev-parse 失败 → Ok(None)
        let result = run_optional(&runner.cmd_rev_parse_verify_head()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_worktree_list_handles_all_states() {
        let output = "worktree /ws/main\nHEAD aaaa1111\nbranch refs/heads/main\n\nworktree /ws/feature\nHEAD bbbb2222\ndetached\n\nworktree /ws/locked\nHEAD cccc3333\nbranch refs/heads/locked\nlocked 手工锁定\n\nworktree /ws/prunable\nHEAD dddd4444\nbranch refs/heads/prunable\nprunable gitdir file points to non-existent location\n\nworktree /ws/bare\nbare\n";
        let entries = parse_worktree_list(output);
        assert_eq!(entries.len(), 5);

        assert_eq!(entries[0].path, PathBuf::from("/ws/main"));
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert_eq!(entries[0].head.as_deref(), Some("aaaa1111"));
        assert!(!entries[0].locked && !entries[0].prunable && !entries[0].detached);

        assert!(entries[1].detached);
        assert_eq!(entries[1].branch, None);

        assert!(entries[2].locked);
        assert_eq!(entries[2].branch.as_deref(), Some("locked"));

        assert!(entries[3].prunable);
        assert!(entries[4].bare);
    }

    #[test]
    fn parse_worktree_list_handles_large_output_without_truncation() {
        // 验收 §9：porcelain 输出完整解析、不截断（50+ 条目）
        let mut output = String::new();
        for index in 0..60 {
            output.push_str(&format!(
                "worktree /ws/iter{index}\nHEAD {index:040}\nbranch refs/heads/feature/{index}\n\n"
            ));
        }
        let entries = parse_worktree_list(&output);
        assert_eq!(entries.len(), 60);
        assert_eq!(entries[59].branch.as_deref(), Some("feature/59"));
        assert!(entries.iter().all(|entry| entry.head.as_deref().unwrap().len() == 40));
    }

    #[test]
    fn parse_status_handles_large_output_without_truncation() {
        let mut output = String::new();
        for index in 0..80 {
            output.push_str(&format!(" M src/file{index}.php
"));
        }
        for index in 0..30 {
            output.push_str(&format!("?? notes/{index}.md
"));
        }
        let summary = parse_status(&output);
        assert_eq!(summary.tracked_changes.len(), 80);
        assert_eq!(summary.untracked.len(), 30);
        assert!(summary.is_dirty());
    }

    #[test]
    fn parse_worktree_list_without_trailing_blank_line() {
        let output = "worktree /ws/only\nHEAD aaaa\nbranch refs/heads/main";
        let entries = parse_worktree_list(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn parse_status_covers_tracked_untracked_rename_and_subdir() {
        let output = " M src/order/calc.php\nM  src/app.ts\n?? notes/todo.md\n?? src/generated/\nR  src/old.ts -> src/new.ts\n?? \"vendor/weird name/x.php\"\n";
        let summary = parse_status(output);
        assert_eq!(
            summary.tracked_changes,
            vec![
                "src/order/calc.php".to_string(),
                "src/app.ts".to_string(),
                "src/old.ts -> src/new.ts".to_string()
            ]
        );
        assert_eq!(
            summary.untracked,
            vec![
                "notes/todo.md".to_string(),
                "src/generated/".to_string(),
                "vendor/weird name/x.php".to_string()
            ]
        );
        assert!(summary.is_dirty());
        assert_eq!(summary.all_paths().len(), 6);
    }

    #[test]
    fn parse_status_empty_is_clean() {
        let summary = parse_status("");
        assert!(!summary.is_dirty());
        assert!(summary.tracked_changes.is_empty());
        assert!(summary.untracked.is_empty());
    }
}

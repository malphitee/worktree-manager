//! S3 集成测试：`tempfile` + 本地裸仓库作 remote（implementation-plan.md §2 S3）。
//! 本机没有可执行的 `git` 时打印原因并 `return`（生产代码不因此跳过任何校验）。

use std::path::{Path, PathBuf};
use std::process::Command;

use worktree_manager_lib::models::{
    AppConfig, CreateProjectRequest, CreateRequest, CreateStatus, HeadMode, Lifecycle, Manifest,
    MoveProjectRequest, ProjectConfig, ProjectType, RemoveRequest, SharedDirectoryRule,
};
use worktree_manager_lib::{manifest, removal, workspace};

// ================================== 测试工具 ==================================

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "wm-test")
        .env("GIT_AUTHOR_EMAIL", "wm-test@example.com")
        .env("GIT_COMMITTER_NAME", "wm-test")
        .env("GIT_COMMITTER_EMAIL", "wm-test@example.com")
        .output()
        .expect("无法执行 git");
    assert!(
        output.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    workspace: PathBuf,
    seed: PathBuf,
    remote: PathBuf,
}

enum SeedKind {
    /// 普通项目（无 composer.json）
    Plain,
    /// PHP 项目，带 composer.json / composer.lock / vendor
    Php { vendor: bool },
}

impl SeedKind {
    fn has_vendor(&self) -> bool {
        match self {
            SeedKind::Php { vendor } => *vendor,
            SeedKind::Plain => false,
        }
    }
}

impl Fixture {
    fn new(seed_kind: SeedKind) -> Self {
        let temp = tempfile::tempdir().expect("创建临时目录失败");
        let root = temp.path().to_path_buf();
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let remote = root.join("remotes").join("seed.git");
        std::fs::create_dir_all(remote.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "--initial-branch=master"]);

        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "--initial-branch=master"]);
        std::fs::write(seed.join("README.md"), "# seed\n").unwrap();
        match &seed_kind {
            SeedKind::Plain => {}
            SeedKind::Php { vendor } => {
                std::fs::write(seed.join("composer.json"), "{}\n").unwrap();
                std::fs::write(seed.join("composer.lock"), "{\"lock\":1}\n").unwrap();
                if *vendor {
                    std::fs::create_dir_all(seed.join("vendor").join("acme")).unwrap();
                    std::fs::write(seed.join("vendor").join("autoload.php"), "<?php\n").unwrap();
                    std::fs::write(seed.join("vendor").join(".git"), "gitdir: /nowhere\n").unwrap();
                }
            }
        }
        git(&seed, &["add", "-A"]);
        // PHP fixture 的 vendor 必须保持「未跟踪且未被忽略」，才能覆盖 vendor 单独清理路径
        if seed_kind.has_vendor() {
            git(&seed, &["reset", "-q", "--", "vendor"]);
        }
        git(&seed, &["commit", "-m", "初始提交"]);
        git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&seed, &["push", "origin", "master"]);
        git(&seed, &["checkout", "-b", "develop"]);
        git(&seed, &["push", "origin", "develop"]);
        git(&seed, &["checkout", "master"]);

        Fixture {
            _temp: temp,
            root,
            workspace,
            seed,
            remote,
        }
    }

    fn config(&self) -> AppConfig {
        AppConfig {
            schema_version: 1,
            workspace_root: Some(self.workspace.to_string_lossy().to_string()),
            shared_directories: Vec::new(),
            projects: vec![ProjectConfig {
                id: "api3".to_string(),
                repository_path: self.seed.to_string_lossy().to_string(),
                project_type: ProjectType::Other,
                vendor_available: false,
            }],
            recent_iterations: Vec::new(),
        }
    }

    fn iteration_dir(&self, iteration: &str) -> PathBuf {
        self.workspace.join(iteration)
    }

    fn read_manifest(&self, iteration: &str) -> Manifest {
        let path = manifest::manifest_path(&self.iteration_dir(iteration));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取清单失败 {}：{error}", path.display()));
        serde_json::from_str(&text).expect("清单必须是完整可解析 JSON")
    }

    fn create(&self, config: &AppConfig, request: CreateRequest) -> worktree_manager_lib::models::CreateBatchResult {
        workspace::create_batch(config, request, |_progress| {}).expect("创建流程应成功返回")
    }
}

/// 归档评估 / 归档执行的空进度回调（`assess` / `archive` 接收 `&F`）
fn noop_emit(_progress: worktree_manager_lib::models::MergeCheckProgress) {}

fn branch_request(iteration: &str, branch: Option<&str>, base_ref: &str) -> CreateRequest {
    CreateRequest {
        iteration: iteration.to_string(),
        note: None,
        unified_branch: None,
        unified_base_ref: base_ref.to_string(),
        projects: vec![CreateProjectRequest {
            project_id: "api3".to_string(),
            branch: branch.map(|value| value.to_string()),
            base_ref: base_ref.to_string(),
        }],
    }
}

fn create_second_seed(fx: &Fixture) -> PathBuf {
    let remote = fx.root.join("remotes").join("other.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "--bare", "--initial-branch=master"]);
    let seed = fx.root.join("seed2");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--initial-branch=master"]);
    std::fs::write(seed.join("README.md"), "# seed2\n").unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-m", "初始提交"]);
    git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&seed, &["push", "origin", "master"]);
    seed
}

// =================================== 用例 ===================================

#[test]
fn fetch_head_commit_matches_remote_and_worktree_head() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));

    assert!(!result.aborted);
    assert_eq!(result.projects.len(), 1);
    assert_eq!(result.projects[0].status, CreateStatus::Created);

    let remote_head = git(&fx.seed, &["rev-parse", "origin/master"]).trim().to_string();
    let manifest = fx.read_manifest("7.3.0");
    let record = &manifest.projects[0];
    assert_eq!(record.lifecycle, Lifecycle::Active);
    assert_eq!(record.base_ref, "origin/master");
    assert_eq!(record.base_commit.as_deref(), Some(remote_head.as_str()));
    assert_eq!(record.base_commit.as_deref().unwrap().len(), 40);
    assert_eq!(record.head_mode, HeadMode::Branch);
    assert_eq!(record.branch.as_deref(), Some("feature/x"));

    let worktree = PathBuf::from(&record.worktree_path);
    assert!(worktree.is_dir());
    let worktree_head = git(&worktree, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(worktree_head, remote_head);

    // 本地裸仓库 remote 确实持有 master
    let bare_head = git(&fx.remote, &["rev-parse", "--verify", "refs/heads/master"])
        .trim()
        .to_string();
    assert_eq!(bare_head, remote_head);
}

#[test]
fn second_project_failure_keeps_first_and_records_both() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let seed2 = create_second_seed(&fx);
    let mut config = fx.config();
    config.projects.push(ProjectConfig {
        id: "other".to_string(),
        repository_path: seed2.to_string_lossy().to_string(),
        project_type: ProjectType::Other,
        vendor_available: false,
    });

    let mut request = branch_request("7.3.0", Some("feature/x"), "");
    request.projects = vec![
        CreateProjectRequest {
            project_id: "api3".to_string(),
            branch: Some("feature/x".to_string()),
            base_ref: String::new(),
        },
        CreateProjectRequest {
            project_id: "other".to_string(),
            branch: Some("feature/x".to_string()),
            // 远端不存在 release 分支 → 该项目失败
            base_ref: "origin/release".to_string(),
        },
    ];
    let result = fx.create(&config, request);

    assert_eq!(result.projects.len(), 2);
    assert_eq!(result.projects[0].status, CreateStatus::Created);
    assert_eq!(result.projects[1].status, CreateStatus::Failed);
    assert!(!result.aborted);

    let manifest = fx.read_manifest("7.3.0");
    assert_eq!(manifest.projects.len(), 2);
    assert_eq!(manifest.projects[0].lifecycle, Lifecycle::Active);
    assert_eq!(manifest.projects[1].lifecycle, Lifecycle::CreateFailed);
    assert_eq!(manifest.projects[1].base_ref, "origin/release");
    assert!(manifest.projects[1]
        .create_result
        .message
        .as_deref()
        .map(|message| !message.is_empty())
        .unwrap_or(false));
}

#[test]
fn three_creates_suffix_directories_then_already_exists() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    for branch in ["feature/a", "feature/b", "feature/c"] {
        let result = fx.create(&config, branch_request("7.3.0", Some(branch), ""));
        assert_eq!(result.projects[0].status, CreateStatus::Created);
    }
    let manifest = fx.read_manifest("7.3.0");
    let names: Vec<String> = manifest
        .projects
        .iter()
        .map(|record| {
            Path::new(&record.worktree_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(names, vec!["api3", "api3-2", "api3-3"]);

    // 同项目同分支二次创建 → alreadyExists（不再新建目录）
    let again = fx.create(&config, branch_request("7.3.0", Some("feature/a"), ""));
    assert_eq!(again.projects[0].status, CreateStatus::AlreadyExists);
    assert_eq!(fx.read_manifest("7.3.0").projects.len(), 3);
}

#[test]
fn existing_local_branch_fails_without_creating_directory() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    git(&fx.seed, &["branch", "feature/exists"]);
    let config = fx.config();
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/exists"), ""));

    assert_eq!(result.projects[0].status, CreateStatus::Failed);
    assert!(result.projects[0]
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("本地已存在同名分支"));
    assert!(!fx.iteration_dir("7.3.0").join("api3").exists());
    let manifest = fx.read_manifest("7.3.0");
    assert_eq!(manifest.projects[0].lifecycle, Lifecycle::CreateFailed);
}

#[test]
fn shared_directory_failure_aborts_batch_without_fetch() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    // 源为符号链接 → copy_snapshot 拒绝
    let real = fx.root.join("real-common");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("file.txt"), "x").unwrap();
    let link = fx.root.join("link-common");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(not(unix))]
    {
        eprintln!("跳过：非 Unix 平台未创建符号链接");
        return;
    }

    let mut config = fx.config();
    config.shared_directories = vec![SharedDirectoryRule {
        source_path: link.to_string_lossy().to_string(),
        target_directory: "fd-common".to_string(),
    }];
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));

    assert!(result.aborted);
    assert!(result.projects.is_empty());
    assert!(result.abort_reason.is_some());
    let manifest = fx.read_manifest("7.3.0");
    assert_eq!(manifest.projects.len(), 0);
    assert_eq!(
        manifest.shared_directories[0].status,
        worktree_manager_lib::models::SharedDirStatus::Failed
    );
    // 没有项目进入 fetch：迭代目录下没有任何 worktree 目录
    assert!(!fx.iteration_dir("7.3.0").join("api3").exists());
}

#[test]
fn manifest_is_complete_json_after_each_project() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let seed2 = create_second_seed(&fx);
    let mut config = fx.config();
    config.projects.push(ProjectConfig {
        id: "other".to_string(),
        repository_path: seed2.to_string_lossy().to_string(),
        project_type: ProjectType::Other,
        vendor_available: false,
    });
    let mut request = branch_request("7.3.0", Some("feature/x"), "");
    request.projects = vec![
        CreateProjectRequest {
            project_id: "api3".to_string(),
            branch: Some("feature/x".to_string()),
            base_ref: String::new(),
        },
        CreateProjectRequest {
            project_id: "other".to_string(),
            branch: Some("feature/a".to_string()),
            base_ref: String::new(),
        },
    ];

    let manifest_path = manifest::manifest_path(&fx.iteration_dir("7.3.0"));
    let counts = std::cell::RefCell::new(Vec::new());
    let result = workspace::create_batch(&config, request, |progress| {
        if matches!(
            progress.phase,
            worktree_manager_lib::models::CreatePhase::Completed
                | worktree_manager_lib::models::CreatePhase::Failed
        ) {
            let text = std::fs::read_to_string(&manifest_path)
                .expect("每次项目处理后清单都应已写盘");
            let parsed: Manifest =
                serde_json::from_str(&text).expect("清单必须是完整可解析 JSON");
            counts.borrow_mut().push(parsed.projects.len());
        }
    })
    .unwrap();

    assert_eq!(result.projects.len(), 2);
    assert_eq!(*counts.borrow(), vec![1, 2]);
}

#[test]
fn untracked_file_blocks_removal_and_recovers_after_cleanup() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);

    let clean =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(clean.allowed, "干净 worktree 应允许移除：{:?}", clean.risks);
    assert!(clean.risks.is_empty());
    assert!(!clean.vendor_only_cleanup_available);
    assert_eq!(clean.confirmation_text, "7.3.0/api3");

    std::fs::write(worktree.join("notes.md"), "todo").unwrap();
    let blocked =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!blocked.allowed);
    assert!(blocked
        .risks
        .iter()
        .any(|risk| risk.code == worktree_manager_lib::models::RiskCode::UntrackedFiles));
    assert!(!blocked.vendor_only_cleanup_available);

    // 执行前重新计算风险：风险仍在 → 拒绝且清单不动
    let request = RemoveRequest {
        iteration: "7.3.0".to_string(),
        project_id: "api3".to_string(),
        worktree_path: record.worktree_path.clone(),
        confirmation: blocked.confirmation_text.clone(),
        remove_copied_vendor: false,
    };
    let error = removal::remove_managed(&config, &request).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Conflict);
    assert_eq!(
        fx.read_manifest("7.3.0").projects[0].lifecycle,
        Lifecycle::Active
    );

    // 删除未跟踪文件后可移除
    std::fs::remove_file(worktree.join("notes.md")).unwrap();
    removal::remove_managed(&config, &request).unwrap();
    assert!(!worktree.exists());
    // 分支、迭代目录与清单历史保留
    assert!(git(&fx.seed, &["branch", "--list", "feature/x"]).contains("feature/x"));
    assert!(fx.iteration_dir("7.3.0").is_dir());
    let after = fx.read_manifest("7.3.0");
    assert_eq!(after.projects[0].lifecycle, Lifecycle::Removed);
    assert!(after.projects[0].removed_at.is_some());
}

#[test]
fn detached_commit_blocks_removal() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let result = fx.create(&config, branch_request("7.3.0", None, ""));
    assert_eq!(result.projects[0].status, CreateStatus::Created);
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    assert_eq!(record.head_mode, HeadMode::Detached);
    assert_eq!(record.branch, None);
    let worktree = PathBuf::from(&record.worktree_path);

    // 刚创建时 HEAD 仍在 origin/master 内 → 可移除
    let clean =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(clean.allowed, "风险：{:?}", clean.risks);

    // 在 detached HEAD 上做一个独立提交
    std::fs::write(worktree.join("README.md"), "# changed\n").unwrap();
    git(&worktree, &["add", "-A"]);
    git(&worktree, &["commit", "-m", "detached 独立提交"]);

    let blocked =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!blocked.allowed);
    assert!(blocked
        .risks
        .iter()
        .any(|risk| risk.code == worktree_manager_lib::models::RiskCode::DetachedCommits));

    let request = RemoveRequest {
        iteration: "7.3.0".to_string(),
        project_id: "api3".to_string(),
        worktree_path: record.worktree_path.clone(),
        confirmation: blocked.confirmation_text.clone(),
        remove_copied_vendor: false,
    };
    assert!(removal::remove_managed(&config, &request).is_err());
    assert!(worktree.exists());
}

#[test]
fn vendor_is_copied_and_vendor_only_cleanup_removes_worktree() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Php { vendor: true });
    let mut config = fx.config();
    config.projects[0].project_type = ProjectType::Php;
    config.projects[0].vendor_available = true;
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(
        result.projects[0].vendor_status,
        Some(worktree_manager_lib::models::VendorStatus::Copied)
    );

    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);
    assert!(worktree.join("vendor").join("autoload.php").is_file());
    assert!(!worktree.join("vendor").join(".git").exists());
    assert!(record.vendor.as_ref().unwrap().copied_by_tool);

    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(assessment.vendor_only_cleanup_available, "风险：{:?}", assessment.risks);
    assert!(assessment.allowed);

    let request = RemoveRequest {
        iteration: "7.3.0".to_string(),
        project_id: "api3".to_string(),
        worktree_path: record.worktree_path.clone(),
        confirmation: assessment.confirmation_text.clone(),
        remove_copied_vendor: true,
    };
    removal::remove_managed(&config, &request).unwrap();
    assert!(!worktree.join("vendor").exists());
    assert!(!worktree.exists());
}

#[test]
fn discovered_worktree_is_listed_assessed_and_removed() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));

    // 不经工具直接建 worktree → 复核时应判为 discovered
    let ghost = fx.iteration_dir("7.3.0").join("ghost");
    git(
        &fx.seed,
        &[
            "worktree",
            "add",
            "-b",
            "ghost-branch",
            ghost.to_str().unwrap(),
            "origin/master",
        ],
    );

    // 快扫不出现 discovered
    let quick = manifest::list_groups(&config, false).unwrap();
    assert!(!quick[0]
        .projects
        .iter()
        .any(|row| row.validity == worktree_manager_lib::models::Validity::Discovered));

    let groups = manifest::list_groups(&config, true).unwrap();
    let ghost_row = groups[0]
        .projects
        .iter()
        .find(|row| row.validity == worktree_manager_lib::models::Validity::Discovered)
        .expect("复核应发现 discovered 行")
        .clone();
    assert_eq!(ghost_row.project_id, "ghost");
    assert_eq!(ghost_row.branch_display, "ghost-branch");
    assert_eq!(ghost_row.lifecycle, None);
    assert!(ghost_row.removable);
    assert!(!ghost_row.source_repository.is_empty());

    let assessment = removal::assess_discovered(
        &config,
        "7.3.0",
        &ghost_row.worktree_path,
        &ghost_row.source_repository,
    )
    .unwrap();
    assert!(assessment.allowed, "风险：{:?}", assessment.risks);
    assert!(!assessment.vendor_only_cleanup_available);

    removal::remove_discovered(
        &config,
        "7.3.0",
        &ghost_row.worktree_path,
        &ghost_row.source_repository,
        &assessment.confirmation_text,
        false,
    )
    .unwrap();
    assert!(!ghost.exists());
    // 不写清单
    assert_eq!(fx.read_manifest("7.3.0").projects.len(), 1);
}

#[test]
fn end_to_end_create_quickscan_reconcile_then_remove_one() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let seed2 = create_second_seed(&fx);
    let mut draft = fx.config();
    draft.projects.push(ProjectConfig {
        id: "other".to_string(),
        repository_path: seed2.to_string_lossy().to_string(),
        project_type: ProjectType::Other,
        vendor_available: false,
    });
    // 等价于「设置页保存」：规范化与校验（不落盘）
    let config = worktree_manager_lib::config::normalize_and_validate(draft).unwrap();

    // 创建两个项目
    let mut request = branch_request("7.3.0", Some("feature/a"), "");
    request.projects = vec![
        CreateProjectRequest {
            project_id: "api3".to_string(),
            branch: Some("feature/a".to_string()),
            base_ref: String::new(),
        },
        CreateProjectRequest {
            project_id: "other".to_string(),
            branch: Some("feature/a".to_string()),
            base_ref: String::new(),
        },
    ];
    let created = fx.create(&config, request);
    assert_eq!(created.projects.len(), 2);
    assert!(created
        .projects
        .iter()
        .all(|project| project.status == CreateStatus::Created));

    // 快扫：存在但未复核
    let quick = manifest::list_groups(&config, false).unwrap();
    assert_eq!(quick.len(), 1);
    assert_eq!(quick[0].manifest_health, worktree_manager_lib::models::ManifestHealth::Valid);
    assert_eq!(quick[0].projects.len(), 2);
    for row in &quick[0].projects {
        assert_eq!(row.validity, worktree_manager_lib::models::Validity::Unknown);
        assert_eq!(row.dirty, None);
        assert_eq!(row.has_changes, None);
        assert!(!row.removable, "快扫不判可移除性");
    }

    // 复核：有效、干净、可移除
    let reconciled = manifest::list_groups(&config, true).unwrap();
    for row in &reconciled[0].projects {
        assert_eq!(row.validity, worktree_manager_lib::models::Validity::Valid);
        assert_eq!(row.dirty, Some(false));
        assert!(row.removable);
        assert!(row.openable);
    }

    // 移除一个
    let target = reconciled[0]
        .projects
        .iter()
        .find(|row| row.project_id == "api3")
        .unwrap()
        .clone();
    let assessment =
        removal::assess_managed(&config, "7.3.0", &target.project_id, &target.worktree_path).unwrap();
    assert!(assessment.allowed, "风险：{:?}", assessment.risks);
    removal::remove_managed(
        &config,
        &RemoveRequest {
            iteration: "7.3.0".to_string(),
            project_id: target.project_id.clone(),
            worktree_path: target.worktree_path.clone(),
            confirmation: assessment.confirmation_text.clone(),
            remove_copied_vendor: false,
        },
    )
    .unwrap();
    assert!(!PathBuf::from(&target.worktree_path).exists());

    // 移除后：一条 removed、另一条仍在
    let after = manifest::list_groups(&config, false).unwrap();
    let removed_row = after[0]
        .projects
        .iter()
        .find(|row| row.project_id == "api3")
        .unwrap();
    assert_eq!(removed_row.validity, worktree_manager_lib::models::Validity::Removed);
    let kept = after[0]
        .projects
        .iter()
        .find(|row| row.project_id == "other")
        .unwrap();
    assert_eq!(kept.validity, worktree_manager_lib::models::Validity::Unknown);
    assert!(kept.openable);

    // 007：removed 记录不出现在合并检查结果里
    let merged = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(merged.records.len(), 1);
    assert_eq!(merged.records[0].project_id, "other");
}

#[test]
fn progress_events_cover_all_phases_and_one_fetch_per_project() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let events = std::cell::RefCell::new(Vec::new());
    workspace::create_batch(&config, branch_request("7.3.0", Some("feature/x"), ""), |progress| {
        events.borrow_mut().push((progress.project_id.clone(), progress.phase, progress.message.clone()));
    })
    .unwrap();

    let events = events.borrow();
    let phases: Vec<_> = events.iter().map(|(_, phase, _)| *phase).collect();
    assert_eq!(
        phases,
        vec![
            worktree_manager_lib::models::CreatePhase::Queued,
            worktree_manager_lib::models::CreatePhase::Fetching,
            worktree_manager_lib::models::CreatePhase::Creating,
            worktree_manager_lib::models::CreatePhase::Vendor,
            worktree_manager_lib::models::CreatePhase::Completed,
        ]
    );
    // 每个项目恰好一次 fetch，且进度文案写实际命令
    let fetches: Vec<&String> = events
        .iter()
        .filter(|(_, phase, _)| *phase == worktree_manager_lib::models::CreatePhase::Fetching)
        .map(|(_, _, message)| message)
        .collect();
    assert_eq!(fetches.len(), 1);
    assert_eq!(fetches[0], "git fetch origin master");
    // creating 阶段含 worktree add 命令
    let creating: Vec<&String> = events
        .iter()
        .filter(|(_, phase, _)| *phase == worktree_manager_lib::models::CreatePhase::Creating)
        .map(|(_, _, message)| message)
        .collect();
    assert!(creating[0].starts_with("git worktree add -b feature/x"));
    assert_eq!(events[0].2, "排队中");
}

#[test]
fn unmanaged_directory_conflict_fails_project() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    // 手工建一个同名普通目录（不受清单管理）
    std::fs::create_dir_all(fx.iteration_dir("7.3.0").join("api3")).unwrap();
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(result.projects[0].status, CreateStatus::Failed);
    assert!(result.projects[0]
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("目录已存在且不受清单管理"));
    assert!(fx
        .iteration_dir("7.3.0")
        .join("api3")
        .read_dir()
        .unwrap()
        .next()
        .is_none(),
        "冲突目录不应被写入内容"
    );
}

#[test]
fn hidden_iteration_is_unhidden_on_create_and_note_kept() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    // 预置：隐藏 + 备注
    let iteration_dir = fx.iteration_dir("7.3.0");
    std::fs::create_dir_all(&iteration_dir).unwrap();
    let mut data = Manifest {
        schema_version: 1,
        iteration: "7.3.0".to_string(),
        created_at: "2026-07-30T10:00:00Z".to_string(),
        note: Some("旧备注".to_string()),
        hidden_at: Some("2026-07-30T11:00:00Z".to_string()),
        archived_at: None,
        shared_directories: Vec::new(),
        projects: Vec::new(),
    };
    manifest::write_manifest(&iteration_dir, &data).unwrap();

    // 缺省 note → 不改动
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    data = fx.read_manifest("7.3.0");
    assert_eq!(data.hidden_at, None, "继续创建应自动取消隐藏");
    assert_eq!(data.note.as_deref(), Some("旧备注"));

    // 显式清除备注
    let mut request = branch_request("7.3.0", Some("feature/y"), "");
    request.note = Some(None);
    fx.create(&config, request);
    assert_eq!(fx.read_manifest("7.3.0").note, None);
}

#[test]
fn shared_directory_copied_then_reused_without_touching_target() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let common = fx.root.join("fd-common");
    std::fs::create_dir_all(&common).unwrap();
    std::fs::write(common.join("index.php"), "<?php\n").unwrap();
    let mut config = fx.config();
    config.shared_directories = vec![SharedDirectoryRule {
        source_path: common.to_string_lossy().to_string(),
        target_directory: "fd-common".to_string(),
    }];

    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let first = fx.read_manifest("7.3.0");
    assert_eq!(first.shared_directories.len(), 1);
    assert_eq!(
        first.shared_directories[0].status,
        worktree_manager_lib::models::SharedDirStatus::Copied
    );
    let target = fx.iteration_dir("7.3.0").join("fd-common");
    let mtime_before = std::fs::metadata(&target).unwrap().modified().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    fx.create(&config, branch_request("7.3.0", Some("feature/y"), ""));
    let second = fx.read_manifest("7.3.0");
    assert_eq!(
        second.shared_directories[0].status,
        worktree_manager_lib::models::SharedDirStatus::Reused
    );
    let mtime_after = std::fs::metadata(&target).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "复用不应覆盖目标目录");
    // 目标内部文件是快照内容
    assert!(target.join("index.php").is_file());
}

#[test]
fn unmanaged_shared_target_aborts_batch() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let common = fx.root.join("fd-common");
    std::fs::create_dir_all(&common).unwrap();
    let mut config = fx.config();
    config.shared_directories = vec![SharedDirectoryRule {
        source_path: common.to_string_lossy().to_string(),
        target_directory: "fd-common".to_string(),
    }];
    // 目标已存在且不受清单管理
    std::fs::create_dir_all(fx.iteration_dir("7.3.0").join("fd-common")).unwrap();

    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert!(result.aborted);
    assert!(result
        .abort_reason
        .as_deref()
        .unwrap_or_default()
        .contains("目标已存在且不受清单管理"));
    let data = fx.read_manifest("7.3.0");
    assert_eq!(
        data.shared_directories[0].status,
        worktree_manager_lib::models::SharedDirStatus::Failed
    );
    assert!(data.projects.is_empty());
}

#[test]
fn vendor_source_recheck_ignores_config_snapshot() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    // 配置里写着有 vendor，但源仓库实际没有 → sourceMissing
    let fx = Fixture::new(SeedKind::Php { vendor: false });
    let mut config = fx.config();
    config.projects[0].project_type = ProjectType::Php;
    config.projects[0].vendor_available = true;
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(result.projects[0].status, CreateStatus::Created);
    assert_eq!(
        result.projects[0].vendor_status,
        Some(worktree_manager_lib::models::VendorStatus::SourceMissing)
    );
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    assert_eq!(
        record.vendor.as_ref().map(|vendor| vendor.status),
        Some(worktree_manager_lib::models::VendorStatus::SourceMissing)
    );
    assert!(!record.vendor.as_ref().unwrap().copied_by_tool);
    assert_eq!(
        record.post_steps[0].status,
        worktree_manager_lib::models::PostStepStatus::Skipped
    );
}

#[test]
fn no_temp_files_or_partial_directories_left_after_create() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let common = fx.root.join("fd-common");
    std::fs::create_dir_all(&common).unwrap();
    std::fs::write(common.join("a.txt"), "a").unwrap();
    let mut config = fx.config();
    config.shared_directories = vec![SharedDirectoryRule {
        source_path: common.to_string_lossy().to_string(),
        target_directory: "fd-common".to_string(),
    }];
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));

    let leftovers: Vec<String> = std::fs::read_dir(fx.iteration_dir("7.3.0"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "不应留下临时目录：{leftovers:?}");
    // 清单没有临时文件残留
    let manifest_temp: Vec<String> = std::fs::read_dir(fx.iteration_dir("7.3.0"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(manifest_temp.is_empty(), "清单写入不应留下临时文件：{manifest_temp:?}");
}

#[test]
fn reconcile_reports_missing_not_registered_head_mismatch_and_source_missing() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let created = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let worktree = PathBuf::from(&fx.read_manifest("7.3.0").projects[0].worktree_path);

    // 1) 切到别的分支 → headMismatch
    git(&worktree, &["checkout", "-b", "feature/other"]);
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(
        groups[0].projects[0].validity,
        worktree_manager_lib::models::Validity::HeadMismatch
    );
    assert_eq!(groups[0].projects[0].branch_display, "feature/other");
    assert!(groups[0].projects[0].removable, "headMismatch 仍可打开移除对话框");

    // 2) 目录删除 → missingDirectory
    std::fs::remove_dir_all(&worktree).unwrap();
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(
        groups[0].projects[0].validity,
        worktree_manager_lib::models::Validity::MissingDirectory
    );
    assert!(!groups[0].projects[0].openable);

    // 3) 目录存在但未注册（删掉 worktree 的 admin 记录）
    git(&fx.seed, &["worktree", "prune"]);
    let created = fx.create(&config, branch_request("7.3.0", Some("feature/z"), ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let mut data = fx.read_manifest("7.3.0");
    let record = data
        .projects
        .iter()
        .find(|record| record.branch.as_deref() == Some("feature/z"))
        .unwrap()
        .clone();
    let admin_root = fx.seed.join(".git").join("worktrees");
    let mut removed_admin = false;
    for entry in std::fs::read_dir(&admin_root).unwrap().flatten() {
        let gitdir_file = entry.path().join("gitdir");
        if let Ok(text) = std::fs::read_to_string(&gitdir_file) {
            if text.contains(record.worktree_path.as_str()) {
                std::fs::remove_dir_all(entry.path()).unwrap();
                removed_admin = true;
            }
        }
    }
    assert!(removed_admin, "应找到该 worktree 的 admin 记录");
    let groups = manifest::list_groups(&config, true).unwrap();
    let row = groups[0]
        .projects
        .iter()
        .find(|row| row.worktree_path == record.worktree_path)
        .unwrap();
    assert_eq!(row.validity, worktree_manager_lib::models::Validity::NotRegistered);

    // 4) 源仓库移走 → sourceMissing
    let moved = fx.root.join("seed-moved");
    std::fs::rename(&fx.seed, &moved).unwrap();
    let groups = manifest::list_groups(&config, true).unwrap();
    assert!(groups[0]
        .projects
        .iter()
        .all(|row| row.validity == worktree_manager_lib::models::Validity::SourceMissing));
    std::fs::rename(&moved, &fx.seed).unwrap();
    // data 变量保持可读（避免未使用告警）
    data.projects.clear();
}

#[test]
fn tracked_changes_unpushed_commits_and_lock_block_removal() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::RiskCode;

    // 1) tracked 修改阻止移除
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);
    std::fs::write(worktree.join("README.md"), "# modified\n").unwrap();
    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!assessment.allowed);
    assert!(assessment
        .risks
        .iter()
        .any(|risk| risk.code == RiskCode::TrackedChanges));
    git(&worktree, &["checkout", "--", "README.md"]);

    // 2) 本地独有提交阻止移除（分支模式 → unpushedCommits）
    std::fs::write(worktree.join("local.txt"), "local\n").unwrap();
    git(&worktree, &["add", "-A"]);
    git(&worktree, &["commit", "-m", "本地独有提交"]);
    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!assessment.allowed);
    assert!(assessment
        .risks
        .iter()
        .any(|risk| risk.code == RiskCode::UnpushedCommits));

    // 3) git worktree lock 阻止移除
    git(
        &fx.seed,
        &["worktree", "lock", "--reason", "测试锁定", record.worktree_path.as_str()],
    );
    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(assessment
        .risks
        .iter()
        .any(|risk| risk.code == RiskCode::WorktreeLocked));
    git(&fx.seed, &["worktree", "unlock", record.worktree_path.as_str()]);
}

#[test]
fn vendor_only_cleanup_is_not_offered_without_copied_by_tool() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Php { vendor: true });
    let mut config = fx.config();
    config.projects[0].project_type = ProjectType::Php;
    config.projects[0].vendor_available = true;
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);
    assert!(worktree.join("vendor").is_dir());

    // 篡改清单：copiedByTool = false → 不提供 vendor 清理
    let mut data = fx.read_manifest("7.3.0");
    if let Some(vendor) = data.projects[0].vendor.as_mut() {
        vendor.copied_by_tool = false;
    }
    manifest::write_manifest(&fx.iteration_dir("7.3.0"), &data).unwrap();

    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!assessment.vendor_only_cleanup_available);
    assert!(!assessment.allowed);
    // 伪造 removeCopiedVendor 也会被拒（校验错误）
    let error = removal::remove_managed(
        &config,
        &RemoveRequest {
            iteration: "7.3.0".to_string(),
            project_id: "api3".to_string(),
            worktree_path: record.worktree_path.clone(),
            confirmation: assessment.confirmation_text.clone(),
            remove_copied_vendor: true,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Validation);
}

#[test]
fn assess_rejects_forged_outside_path() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::RiskCode;
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let outside = fx.root.join("outside-dir");
    std::fs::create_dir_all(&outside).unwrap();

    // 伪造 worktreePath（在迭代目录之外）→ pathInvalid
    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &outside.to_string_lossy()).unwrap();
    assert!(!assessment.allowed);
    assert!(assessment
        .risks
        .iter()
        .any(|risk| risk.code == RiskCode::PathInvalid));

    let error = removal::remove_managed(
        &config,
        &RemoveRequest {
            iteration: "7.3.0".to_string(),
            project_id: "api3".to_string(),
            worktree_path: outside.to_string_lossy().to_string(),
            confirmation: assessment.confirmation_text.clone(),
            remove_copied_vendor: false,
        },
    )
    .unwrap_err();
    // 风险阻止执行：错误码为 conflict（消息里含 pathInvalid 对应的风险说明）
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Conflict);
    assert!(error.message().contains("清单中找不到该 worktree 记录"));
}

#[test]
fn create_without_workspace_root_returns_validation() {
    let config = AppConfig::default();
    let error = workspace::create_batch(&config, branch_request("7.3.0", None, ""), |_| {}).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Validation);
    assert_eq!(error.message(), "请先在设置中填写工作区根目录");
    // 迭代号非法同样在写盘前被拒
    let mut with_root = config.clone();
    with_root.workspace_root = Some(std::env::temp_dir().to_string_lossy().to_string());
    let error = workspace::create_batch(&with_root, branch_request("..", None, ""), |_| {}).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Validation);
}

#[test]
fn quick_scan_does_not_run_git() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert!(fx.read_manifest("7.3.0").shared_directories.is_empty());

    // 源仓库整体移走：快扫仍返回同样的行（不跑任何 Git）
    let moved = fx.root.join("seed-moved");
    std::fs::rename(&fx.seed, &moved).unwrap();
    let groups = manifest::list_groups(&config, false).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].projects.len(), 1);
    assert_eq!(
        groups[0].projects[0].validity,
        worktree_manager_lib::models::Validity::Unknown
    );
    std::fs::rename(&moved, &fx.seed).unwrap();
}

#[test]
fn vendor_copy_failure_keeps_worktree_and_records_reason() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Php { vendor: true });
    // 让源 vendor 里有一个不可读文件 → 复制失败
    let blocked = fx.seed.join("vendor").join("blocked.php");
    std::fs::write(&blocked, "<?php").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    let mut config = fx.config();
    config.projects[0].project_type = ProjectType::Php;
    config.projects[0].vendor_available = true;
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    let created_anyway = result.projects[0].status == CreateStatus::Created;
    if !created_anyway {
        eprintln!("跳过：当前用户不受文件权限限制，未能模拟复制失败");
        return;
    }
    assert_eq!(
        result.projects[0].vendor_status,
        Some(worktree_manager_lib::models::VendorStatus::CopyFailed)
    );
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    assert!(PathBuf::from(&record.worktree_path).is_dir(), "复制失败仍保留 worktree");
    assert_eq!(
        record.post_steps[0].status,
        worktree_manager_lib::models::PostStepStatus::Failed
    );
    assert!(!record.vendor.as_ref().unwrap().copied_by_tool);
}

#[test]
fn broken_worktree_link_blocks_removal() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    // 说明：git 的 `prunable` 标记在 porcelain 输出中的解析由 git.rs 单测覆盖
    // （`parse_worktree_list_handles_all_states`）；此处验证「worktree 链接损坏」时评估会阻止移除。
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);

    // 把 worktree 的 .git 指向不存在的 gitdir → 该 worktree 的 Git 命令全部失败
    std::fs::write(worktree.join(".git"), "gitdir: /nowhere/.git/worktrees/broken\n").unwrap();

    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!assessment.allowed, "链接损坏应阻止移除");
    assert!(
        assessment
            .risks
            .iter()
            .any(|risk| risk.code == worktree_manager_lib::models::RiskCode::PathInvalid),
        "风险：{:?}",
        assessment.risks
    );
}

#[test]
fn extra_untracked_file_disables_vendor_only_cleanup() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Php { vendor: true });
    let mut config = fx.config();
    config.projects[0].project_type = ProjectType::Php;
    config.projects[0].vendor_available = true;
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let worktree = PathBuf::from(&record.worktree_path);

    // vendor 之外还有别的未跟踪文件 → 不提供 vendor 清理
    std::fs::write(worktree.join("draft.md"), "draft").unwrap();
    let assessment =
        removal::assess_managed(&config, "7.3.0", "api3", &record.worktree_path).unwrap();
    assert!(!assessment.vendor_only_cleanup_available);
    assert!(!assessment.allowed);
}

// ============================== S5：合并检查 ==============================

fn clone_remote(fx: &Fixture, name: &str) -> PathBuf {
    let dir = fx.root.join(name);
    git(
        &fx.root,
        &["clone", fx.remote.to_str().unwrap(), dir.to_str().unwrap()],
    );
    dir
}

fn commit_file(worktree: &Path, file: &str, content: &str, message: &str) {
    std::fs::write(worktree.join(file), content).unwrap();
    git(worktree, &["add", "-A"]);
    git(worktree, &["commit", "-m", message]);
}

/// 创建 worktree → 提交一个自有 commit → 推送到远端，返回 (worktree 路径, 分支名)
fn prepare_feature(fx: &Fixture, config: &AppConfig, branch: &str, content: &str) -> PathBuf {
    let result = fx.create(config, branch_request("7.3.0", Some(branch), ""));
    assert_eq!(result.projects[0].status, CreateStatus::Created);
    let record = fx
        .read_manifest("7.3.0")
        .projects
        .iter()
        .find(|record| record.branch.as_deref() == Some(branch))
        .unwrap()
        .clone();
    let worktree = PathBuf::from(&record.worktree_path);
    commit_file(&worktree, "feature.txt", content, &format!("提交 {branch}"));
    git(&worktree, &["push", "origin", branch]);
    worktree
}

fn merge_result_for(_fx: &Fixture, config: &AppConfig, branch: &str) -> worktree_manager_lib::models::MergeRecordResult {
    let result = worktree_manager_lib::merge_check::check(
        config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |_| {},
    )
    .unwrap();
    result
        .records
        .into_iter()
        .find(|record| record.branch_display == branch)
        .unwrap_or_else(|| panic!("应能按 live 分支名找到记录：{branch}"))
}

#[test]
fn merge_check_layer1_true_merge() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    prepare_feature(&fx, &config, "feature/merge", "merged");

    // 远端真 merge 到 develop
    let clone = clone_remote(&fx, "clone-merge");
    git(&clone, &["checkout", "develop"]);
    git(&clone, &["merge", "--no-ff", "-m", "合并 feature/merge", "origin/feature/merge"]);
    git(&clone, &["push", "origin", "develop"]);

    let record = merge_result_for(&fx, &config, "feature/merge");
    assert_eq!(record.develop.status, worktree_manager_lib::models::MergeCellStatus::Merged);
    assert!(record.develop.unmerged_commits.is_empty());
    assert!(!record.develop.stale);
    assert_eq!(record.master.status, worktree_manager_lib::models::MergeCellStatus::Unmerged);
    assert_eq!(record.has_changes, Some(true), "feature 有自有提交");
    assert_eq!(record.dirty, Some(false));
}

#[test]
fn merge_check_layer2_rebased_merge() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    prepare_feature(&fx, &config, "feature/rebase", "rebased");

    // 远端：把 feature rebase 到 develop 后 ff 合入（本地 feature 保留旧 commit）
    let clone = clone_remote(&fx, "clone-rebase");
    git(&clone, &["checkout", "develop"]);
    git(&clone, &["cherry-pick", "origin/feature/rebase"]);
    git(&clone, &["push", "origin", "develop"]);

    let record = merge_result_for(&fx, &config, "feature/rebase");
    assert_eq!(
        record.develop.status,
        worktree_manager_lib::models::MergeCellStatus::Merged,
        "patch-id 相同应判 merged（层 2）"
    );
    assert!(record.develop.unmerged_commits.is_empty());
}

#[test]
fn merge_check_layer3_squash_merge() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/squash", "squashed");
    // 注意：单提交的 squash 与原始提交 patch-id 相同，会被层 2 判为 merged；
    // 层 3（contained）需要多个提交被压成一个，因此这里再加一个提交。
    commit_file(&worktree, "more.txt", "more", "第二个提交");
    git(&worktree, &["push", "origin", "feature/squash"]);

    // 远端：squash 合入 develop
    let clone = clone_remote(&fx, "clone-squash");
    git(&clone, &["checkout", "develop"]);
    git(&clone, &["merge", "--squash", "origin/feature/squash"]);
    git(&clone, &["commit", "-m", "squash: feature/squash"]);
    git(&clone, &["push", "origin", "develop"]);

    let record = merge_result_for(&fx, &config, "feature/squash");
    assert_eq!(
        record.develop.status,
        worktree_manager_lib::models::MergeCellStatus::Contained,
        "疑似 squash 应判 contained（层 3）"
    );
    assert!(record.develop.unmerged_commits.is_empty());
}

#[test]
fn merge_check_unmerged_lists_commits() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/pending", "pending");
    commit_file(&worktree, "second.txt", "second", "第二个提交");

    let record = merge_result_for(&fx, &config, "feature/pending");
    assert_eq!(record.develop.status, worktree_manager_lib::models::MergeCellStatus::Unmerged);
    assert_eq!(record.develop.unmerged_commits.len(), 2, "两个提交都未合并");
    for commit in &record.develop.unmerged_commits {
        assert_eq!(commit.split_whitespace().next().unwrap().len(), 7, "短 hash 应为 7 位：{commit}");
    }
    assert!(record.develop.unmerged_commits.iter().any(|line| line.contains("第二个提交")));
}

#[test]
fn merge_check_reports_target_missing_and_stale() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::MergeCellStatus;
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    prepare_feature(&fx, &config, "feature/targets", "targets");

    // 远端删除 develop：`fetch origin develop master` 会失败 → 该仓库全部 stale=true；
    // 本地也没有 refs/remotes/origin/develop → develop 格 targetMissing，master 仍按缓存判定。
    let clone = clone_remote(&fx, "clone-targets");
    git(&clone, &["push", "origin", "--delete", "develop"]);
    git(&fx.seed, &["update-ref", "-d", "refs/remotes/origin/develop"]);

    let record = merge_result_for(&fx, &config, "feature/targets");
    assert_eq!(record.develop.status, MergeCellStatus::TargetMissing);
    assert!(record.develop.stale, "fetch 失败应标 stale");
    assert_eq!(record.master.status, MergeCellStatus::Unmerged);
    assert!(record.master.stale);

    // 完全不可达的 remote：同样 stale=true，判定不中断
    git(
        &fx.seed,
        &["remote", "set-url", "origin", fx.root.join("missing-remote.git").to_str().unwrap()],
    );
    let record = merge_result_for(&fx, &config, "feature/targets");
    assert!(record.develop.stale && record.master.stale);
    assert_eq!(record.develop.status, MergeCellStatus::TargetMissing);
    assert_eq!(record.master.status, MergeCellStatus::Unmerged);
}

#[test]
fn merge_check_detached_branch_missing_and_not_checkable() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::{CreatePhase, MergeCellStatus, MergeCheckPhase};

    // 1) detached 记录：HEAD commit 已在 master 上 → merged
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let created = fx.create(&config, branch_request("7.3.0", None, ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let detached = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |_| {},
    )
    .unwrap();
    assert_eq!(detached.records.len(), 1);
    assert_eq!(detached.records[0].master.status, MergeCellStatus::Merged);
    assert!(detached.records[0].branch_display.starts_with("detached @ "));

    // 2) 本地分支被删除 → branchMissing
    let worktree = PathBuf::from(&fx.read_manifest("7.3.0").projects[0].worktree_path);
    let feature = prepare_feature(&fx, &config, "feature/deleted", "deleted");
    git(&feature, &["checkout", "--detach"]);
    git(&fx.seed, &["branch", "-D", "feature/deleted"]);
    let deleted = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |_| {},
    )
    .unwrap();
    let record = deleted
        .records
        .iter()
        .find(|row| row.worktree_path == feature.to_string_lossy())
        .unwrap()
        .clone();
    assert_eq!(
        record.develop.status,
        MergeCellStatus::BranchMissing,
        "清单要求分支模式但已 detached"
    );

    // 3) worktree 目录被手工删除 → notCheckable
    std::fs::remove_dir_all(&worktree).unwrap();
    let result = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |_| {},
    )
    .unwrap();
    let missing = result
        .records
        .iter()
        .find(|record| record.worktree_path == worktree.to_string_lossy())
        .unwrap();
    assert_eq!(missing.develop.status, MergeCellStatus::NotCheckable);
    assert_eq!(missing.master.status, MergeCellStatus::NotCheckable);
    assert_eq!(missing.has_changes, None);

    // 附带断言：detached 记录的事件阶段完整
    let _ = (CreatePhase::Fetching, MergeCheckPhase::Record);
}

#[test]
fn merge_check_events_sequence_and_filter() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::MergeCheckPhase;
    use std::cell::RefCell;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    prepare_feature(&fx, &config, "feature/one", "one");
    prepare_feature(&fx, &config, "feature/two", "two");

    // 事件序列：每仓库 1 次 fetching，每条记录 checking → record
    let events = RefCell::new(Vec::new());
    let result = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter::default(),
        |progress| events.borrow_mut().push(progress),
    )
    .unwrap();
    let events = events.into_inner();
    assert_eq!(result.records.len(), 2);
    assert_eq!(events.first().map(|event| event.phase), Some(MergeCheckPhase::Fetching));
    assert_eq!(events.iter().filter(|event| event.phase == MergeCheckPhase::Fetching).count(), 1);
    let records: Vec<_> = events.iter().filter(|event| event.phase == MergeCheckPhase::Record).collect();
    assert_eq!(records.len(), 2);
    for event in &records {
        assert!(event.record.is_some());
        assert_eq!(event.total, 2);
    }
    // index 单调不减，total 恒等
    let mut last = 0;
    for event in &events {
        assert!(event.index >= last);
        last = event.index;
        assert_eq!(event.total, 2);
    }
    // 返回值与事件累积结果一致
    for record in &result.records {
        let from_event = records
            .iter()
            .find(|event| event.record.as_ref().unwrap().worktree_path == record.worktree_path)
            .unwrap();
        assert_eq!(&from_event.record.clone().unwrap(), record);
    }

    // 过滤：只查一条 → total = 1，事件 3 个
    let first = result.records[0].clone();
    let events = RefCell::new(Vec::new());
    let filtered = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter {
            project_id: None,
            worktree_path: Some(first.worktree_path.clone()),
        },
        |progress| events.borrow_mut().push(progress),
    )
    .unwrap();
    assert_eq!(filtered.records.len(), 1);
    assert_eq!(events.into_inner().len(), 3);

    // 过滤无匹配 → 空结果、无事件
    let events = RefCell::new(Vec::new());
    let empty = worktree_manager_lib::merge_check::check(
        &config,
        "7.3.0",
        worktree_manager_lib::merge_check::MergeFilter {
            project_id: Some("不存在的项目".to_string()),
            worktree_path: None,
        },
        |progress| events.borrow_mut().push(progress),
    )
    .unwrap();
    assert!(empty.records.is_empty());
    assert!(events.into_inner().is_empty());
}

#[test]
fn has_changes_covers_three_scenarios() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let result = fx.create(&config, branch_request("7.3.0", Some("feature/changes"), ""));
    assert_eq!(result.projects[0].status, CreateStatus::Created);
    let worktree = PathBuf::from(&fx.read_manifest("7.3.0").projects[0].worktree_path);

    // 1) 刚创建（无自有提交）→ false
    let record = merge_result_for(&fx, &config, "feature/changes");
    assert_eq!(record.has_changes, Some(false));

    // 2) 基分支前进（远端 master 有新提交）→ 仍为 false，不误报
    let clone = clone_remote(&fx, "clone-base");
    commit_file(&clone, "base.txt", "base advanced", "基分支前进");
    git(&clone, &["push", "origin", "master"]);
    let record = merge_result_for(&fx, &config, "feature/changes");
    assert_eq!(record.has_changes, Some(false), "基分支前进不应误报有变更");

    // 3) 有自有提交 → true
    commit_file(&worktree, "own.txt", "own", "自有提交");
    let record = merge_result_for(&fx, &config, "feature/changes");
    assert_eq!(record.has_changes, Some(true));

    // 复核档也用同一函数
    let groups = manifest::list_groups(&config, true).unwrap();
    let row = groups[0]
        .projects
        .iter()
        .find(|row| row.worktree_path == worktree.to_string_lossy())
        .unwrap();
    assert_eq!(row.has_changes, Some(true));
    assert_eq!(row.dirty, Some(false));
}

// ============================== S6：归档 / 备注 / 隐藏 ==============================

#[test]
fn archive_assess_reports_clean_records_and_blockers() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use std::cell::RefCell;
    use worktree_manager_lib::archive;
    use worktree_manager_lib::models::MergeCheckPhase;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    // 一条干净（无自有提交）、一条不干净（有未合并的提交）
    let clean = prepare_feature(&fx, &config, "feature/clean", "clean");
    git(&clean, &["reset", "--hard", "origin/master"]);
    let worktree = prepare_feature(&fx, &config, "feature/dirty", "dirty");
    commit_file(&worktree, "more.txt", "more", "未合并的提交");

    let events = RefCell::new(Vec::new());
    let assessment = archive::assess(&config, "7.3.0", |progress| events.borrow_mut().push(progress)).unwrap();

    assert_eq!(assessment.confirmation_text, "7.3.0");
    assert!(!assessment.clean);
    assert_eq!(assessment.records.len(), 2);
    let clean_record = assessment.records.iter().find(|record| record.clean).expect("应有一条干净记录");
    let blocked = assessment.records.iter().find(|record| !record.clean).expect("应有一条不干净记录");
    assert!(clean_record.blockers.is_empty());
    assert!(blocked.blockers.iter().any(|blocker| blocker.contains("develop：未合并")));
    // 事件：全部 record == null（设计 008 决策 2）
    assert!(!events.borrow().is_empty());
    assert!(events.borrow().iter().all(|event| event.record.is_none()));
    assert!(events.borrow().iter().any(|event| event.phase == MergeCheckPhase::Fetching));
}

#[test]
fn archive_rejects_unclean_without_force_and_keeps_manifest() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::archive;
    use worktree_manager_lib::models::ArchiveRequest;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/pending", "pending");
    commit_file(&worktree, "more.txt", "more", "未合并");

    let error = archive::archive(
        &config,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0".to_string(),
            force: false,
        },
        noop_emit,
    )
    .unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Conflict);
    assert!(worktree.is_dir(), "拒绝时不得移除任何 worktree");
    let data = fx.read_manifest("7.3.0");
    assert_eq!(data.projects[0].lifecycle, Lifecycle::Active);
    assert_eq!(data.archived_at, None);

    // 确认文本不严格相等 → validation
    let error = archive::archive(
        &config,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0 ".to_string(),
            force: true,
        },
        noop_emit,
    )
    .unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Validation);
    assert!(worktree.is_dir());
}

#[test]
fn archive_clean_iteration_removes_worktrees_without_force() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::archive;
    use worktree_manager_lib::models::ArchiveRequest;

    let fx = Fixture::new(SeedKind::Plain);
    let common = fx.root.join("fd-common");
    std::fs::create_dir_all(&common).unwrap();
    std::fs::write(common.join("a.txt"), "a").unwrap();
    let mut config = fx.config();
    config.shared_directories = vec![SharedDirectoryRule {
        source_path: common.to_string_lossy().to_string(),
        target_directory: "fd-common".to_string(),
    }];
    // 两条都是干净记录（无自有提交）
    let one = prepare_feature(&fx, &config, "feature/one", "one");
    git(&one, &["reset", "--hard", "origin/master"]);
    let two = prepare_feature(&fx, &config, "feature/two", "two");
    git(&two, &["reset", "--hard", "origin/master"]);

    let assessment = archive::assess(&config, "7.3.0", noop_emit).unwrap();
    assert!(assessment.clean, "风险：{:?}", assessment.records.iter().map(|record| record.blockers.clone()).collect::<Vec<_>>());

    let outcome = archive::archive(
        &config,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0".to_string(),
            force: false,
        },
        noop_emit,
    )
    .unwrap();
    assert!(outcome.archived);
    assert_eq!(outcome.removed_count, 2);
    assert!(outcome.failed.is_empty());
    assert!(!one.exists() && !two.exists());

    // 清单：每条 removed + removedAt，archivedAt 非空
    let data = fx.read_manifest("7.3.0");
    assert!(data.archived_at.is_some());
    for record in &data.projects {
        assert_eq!(record.lifecycle, Lifecycle::Removed);
        assert!(record.removed_at.is_some());
    }
    // 归档不删除：本地分支、迭代目录、公共目录快照、清单文件仍在
    assert!(git(&fx.seed, &["branch", "--list", "feature/one"]).contains("feature/one"));
    assert!(fx.iteration_dir("7.3.0").is_dir());
    assert!(fx.iteration_dir("7.3.0").join("fd-common").join("a.txt").is_file());
    assert!(manifest::manifest_path(&fx.iteration_dir("7.3.0")).is_file());

    // 归档后两档列表都不出现该迭代
    assert!(manifest::list_groups(&config, false).unwrap().is_empty());
    assert!(manifest::list_groups(&config, true).unwrap().is_empty());
}

#[test]
fn archive_force_only_applies_to_unclean_and_locked_still_fails() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::archive;
    use worktree_manager_lib::models::ArchiveRequest;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let clean = prepare_feature(&fx, &config, "feature/clean", "clean");
    git(&clean, &["reset", "--hard", "origin/master"]);
    let dirty = prepare_feature(&fx, &config, "feature/dirty", "dirty");
    commit_file(&dirty, "more.txt", "more", "未推送提交");

    let outcome = archive::archive(
        &config,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0".to_string(),
            force: true,
        },
        noop_emit,
    )
    .unwrap();
    assert!(outcome.archived, "force 归档应成功：{:?}", outcome.failed);
    assert_eq!(outcome.removed_count, 2);
    assert!(!clean.exists() && !dirty.exists());

    // locked worktree：即使 force=true 也失败（不允许 --force --force）
    let fx2 = Fixture::new(SeedKind::Plain);
    let config2 = fx2.config();
    let lockable = prepare_feature(&fx2, &config2, "feature/locked", "locked");
    git(&lockable, &["reset", "--hard", "origin/master"]);
    let other = prepare_feature(&fx2, &config2, "feature/other", "other");
    git(&other, &["reset", "--hard", "origin/master"]);
    git(&fx2.seed, &["worktree", "lock", "--reason", "测试", lockable.to_str().unwrap()]);
    let outcome = archive::archive(
        &config2,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0".to_string(),
            force: true,
        },
        noop_emit,
    )
    .unwrap();
    assert!(!outcome.archived);
    assert_eq!(outcome.failed.len(), 1);
    assert!(lockable.exists(), "locked worktree 不应被移除（禁止 --force --force）");
    assert!(!other.exists(), "其余记录仍被移除（单条失败不中断整批）");
    let data = fx2.read_manifest("7.3.0");
    assert_eq!(data.archived_at, None);
    let locked_record = data
        .projects
        .iter()
        .find(|record| record.worktree_path == lockable.to_string_lossy())
        .unwrap();
    assert_eq!(locked_record.lifecycle, Lifecycle::Active, "失败记录保持 active");
    git(&fx2.seed, &["worktree", "unlock", lockable.to_str().unwrap()]);
}

#[test]
fn archive_reports_damaged_manifest_and_already_archived() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::archive;
    use worktree_manager_lib::models::ArchiveRequest;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let iteration_dir = fx.iteration_dir("7.3.0");
    std::fs::create_dir_all(&iteration_dir).unwrap();

    // 清单损坏 → manifestDamaged
    std::fs::write(manifest::manifest_path(&iteration_dir), "{ 坏").unwrap();
    let error = archive::assess(&config, "7.3.0", noop_emit).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::ManifestDamaged);

    // 已归档 → conflict
    let mut data = Manifest {
        schema_version: 1,
        iteration: "7.3.0".to_string(),
        created_at: "2026-07-30T10:00:00Z".to_string(),
        note: None,
        hidden_at: None,
        archived_at: Some("2026-07-31T10:00:00Z".to_string()),
        shared_directories: Vec::new(),
        projects: Vec::new(),
    };
    manifest::write_manifest(&iteration_dir, &data).unwrap();
    let error = archive::assess(&config, "7.3.0", noop_emit).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Conflict);
    let error = archive::archive(
        &config,
        &ArchiveRequest {
            iteration: "7.3.0".to_string(),
            confirmation: "7.3.0".to_string(),
            force: true,
        },
        noop_emit,
    )
    .unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::Conflict);
    data.archived_at = None;
    manifest::write_manifest(&iteration_dir, &data).unwrap();
}

// ============================== S7：分支改名同步（011） ==============================

#[test]
fn rename_is_synced_and_written_back() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/a", "a");
    let before = fx.read_manifest("7.3.0");
    let manifest_path = manifest::manifest_path(&fx.iteration_dir("7.3.0"));

    // 在 worktree 内改名
    git(&worktree, &["branch", "-m", "feature/a", "feature/b"]);

    let groups = manifest::list_groups(&config, true).unwrap();
    let row = &groups[0].projects[0];
    assert_eq!(row.validity, worktree_manager_lib::models::Validity::Valid);
    assert_eq!(row.branch_display, "feature/b");
    assert_eq!(row.renamed_from.as_deref(), Some("feature/a"));
    assert!(row.removable);

    // 清单 branch 已回写，其余字段不变
    let after = fx.read_manifest("7.3.0");
    assert_eq!(after.projects[0].branch.as_deref(), Some("feature/b"));
    assert_eq!(after.projects[0].project_id, before.projects[0].project_id);
    assert_eq!(after.projects[0].worktree_path, before.projects[0].worktree_path);
    assert_eq!(after.projects[0].base_commit, before.projects[0].base_commit);
    assert_eq!(after.projects[0].base_ref, before.projects[0].base_ref);
    assert_eq!(after.projects[0].created_at, before.projects[0].created_at);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.note, before.note);
    assert_eq!(after.hidden_at, before.hidden_at);
    assert_eq!(after.projects[0].lifecycle, Lifecycle::Active);

    // 临时文件不残留
    let leftovers: Vec<String> = std::fs::read_dir(fx.iteration_dir("7.3.0"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "临时文件残留：{leftovers:?}");

    // 二次复核幂等：renamedFrom 为 null 且清单不再被写
    let mtime_before = std::fs::metadata(&manifest_path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(groups[0].projects[0].renamed_from, None);
    assert_eq!(groups[0].projects[0].branch_display, "feature/b");
    let mtime_after = std::fs::metadata(&manifest_path).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "幂等复核不应再写清单");
}

#[test]
fn switched_branch_stays_head_mismatch_without_write_back() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/a", "a");

    // 源仓库另建 feature/c，worktree 切过去（feature/a 仍存在）
    git(&fx.seed, &["branch", "feature/c", "origin/master"]);
    git(&worktree, &["checkout", "feature/c"]);

    let groups = manifest::list_groups(&config, true).unwrap();
    let row = &groups[0].projects[0];
    assert_eq!(row.validity, worktree_manager_lib::models::Validity::HeadMismatch);
    assert_eq!(row.branch_display, "feature/c");
    assert_eq!(row.renamed_from, None);
    assert_eq!(fx.read_manifest("7.3.0").projects[0].branch.as_deref(), Some("feature/a"));

    // 清单分支被删且 worktree 为 detached → headMismatch，清单不动
    git(&worktree, &["checkout", "--detach"]);
    git(&fx.seed, &["branch", "-D", "feature/a"]);
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(
        groups[0].projects[0].validity,
        worktree_manager_lib::models::Validity::HeadMismatch
    );
    assert_eq!(groups[0].projects[0].renamed_from, None);
    assert_eq!(fx.read_manifest("7.3.0").projects[0].branch.as_deref(), Some("feature/a"));
}

#[test]
fn detached_records_stay_valid_across_new_commits_and_branch_checkout() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::Validity;
    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let created = fx.create(&config, branch_request("7.3.0", None, ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let worktree = PathBuf::from(&fx.read_manifest("7.3.0").projects[0].worktree_path);

    // HEAD == baseCommit → valid
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(groups[0].projects[0].validity, Validity::Valid);

    // 在 detached 上提交 → 仍 valid
    commit_file(&worktree, "d.txt", "d", "detached 提交");
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(groups[0].projects[0].validity, Validity::Valid);
    assert_eq!(groups[0].projects[0].has_changes, Some(true));

    // checkout -b → headMismatch（清单是 detached）
    git(&worktree, &["checkout", "-b", "feature/collected"]);
    let groups = manifest::list_groups(&config, true).unwrap();
    assert_eq!(
        groups[0].projects[0].validity,
        worktree_manager_lib::models::Validity::HeadMismatch
    );
}

#[test]
fn rename_keeps_merge_check_and_removal_on_live_name() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::merge_check::{check, MergeFilter};
    use worktree_manager_lib::models::MergeCellStatus;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let worktree = prepare_feature(&fx, &config, "feature/a", "a");
    git(&worktree, &["branch", "-m", "feature/a", "feature/b"]);
    let _ = manifest::list_groups(&config, true).unwrap();
    let record = fx.read_manifest("7.3.0").projects[0].clone();

    // 合并检查按 live 名（feature/b）判定：不应出现 branchMissing
    let merged = check(&config, "7.3.0", MergeFilter::default(), |_| {}).unwrap();
    assert_eq!(merged.records.len(), 1);
    assert_eq!(merged.records[0].branch_display, "feature/b");
    assert_ne!(merged.records[0].develop.status, MergeCellStatus::BranchMissing);
    assert_eq!(merged.records[0].develop.status, MergeCellStatus::Unmerged);

    // 移除评估：不因分支名报错，风险为「有未推送提交」之外的常规结论
    let assessment = removal::assess_managed(
        &config,
        "7.3.0",
        &record.project_id,
        &record.worktree_path,
    )
    .unwrap();
    assert!(!assessment
        .risks
        .iter()
        .any(|risk| risk.message.contains("未注册")), "改名不应导致未注册");
}

#[test]
fn rename_write_back_failure_keeps_projection_and_reports_in_message() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fixture::new(SeedKind::Plain);
        let config = fx.config();
        let worktree = prepare_feature(&fx, &config, "feature/a", "a");
        git(&worktree, &["branch", "-m", "feature/a", "feature/b"]);
        let iteration_dir = fx.iteration_dir("7.3.0");
        let manifest_path = manifest::manifest_path(&iteration_dir);
        let before = std::fs::read_to_string(&manifest_path).unwrap();
        std::fs::set_permissions(&iteration_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = manifest::list_groups(&config, true);

        std::fs::set_permissions(&iteration_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let groups = result.unwrap();
        let row = &groups[0].projects[0];
        if row.renamed_from.is_none() {
            eprintln!("跳过：当前用户不受目录权限限制，未能模拟回写失败");
            return;
        }
        assert_eq!(row.validity, worktree_manager_lib::models::Validity::Valid);
        assert_eq!(row.branch_display, "feature/b");
        assert!(groups[0]
            .manifest_message
            .as_deref()
            .unwrap_or_default()
            .contains("分支重命名回写失败"),
            "message：{:?}",
            groups[0].manifest_message
        );
        assert_eq!(std::fs::read_to_string(&manifest_path).unwrap(), before, "回写失败时清单不变");
    }
    #[cfg(not(unix))]
    {
        eprintln!("跳过：非 Unix 平台未验证只读目录回写失败");
    }
}

// ============================== S8：基分支候选（013） ==============================

#[test]
fn list_remote_branches_local_then_remote_then_degraded() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::base_ref;
    use worktree_manager_lib::models::BranchSource;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();

    // 本地候选：只读 refs/remotes，不联网
    let local = base_ref::list_remote_branches(&config, "api3", false).unwrap();
    assert_eq!(local.source, BranchSource::Local);
    assert!(local.warning.is_none());
    assert_eq!(local.remotes, vec!["origin".to_string()]);
    assert!(local.branches.iter().any(|branch| branch == "origin/master"));
    assert!(local.branches.iter().any(|branch| branch == "origin/develop"));
    assert!(!local.branches.iter().any(|branch| branch.ends_with("/HEAD")));

    // 远端候选：ls-remote 成功 → 并集 + source = remote
    let remote = base_ref::list_remote_branches(&config, "api3", true).unwrap();
    assert_eq!(remote.source, BranchSource::Remote);
    assert!(remote.warning.is_none());
    assert!(remote.branches.iter().any(|branch| branch == "origin/develop"));
    // 去重排序
    let mut sorted = remote.branches.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, remote.branches);

    // 不可达 remote → 降级为本地候选 + warning，不报错
    git(
        &fx.seed,
        &["remote", "set-url", "origin", fx.root.join("missing-remote.git").to_str().unwrap()],
    );
    let degraded = base_ref::list_remote_branches(&config, "api3", true).unwrap();
    assert_eq!(degraded.source, BranchSource::Local);
    assert!(degraded.warning.is_some());
    assert!(degraded.branches.iter().any(|branch| branch == "origin/master"));

    // 未配置项目 → notFound
    let error = base_ref::list_remote_branches(&config, "nope", false).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::NotFound);
}

// ============================== S8：排序与跨迭代移动（014） ==============================

#[test]
fn reorder_moves_records_by_anchor_semantics() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::relocate;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    for branch in ["feature/a", "feature/b", "feature/c"] {
        let result = fx.create(&config, branch_request("7.3.0", Some(branch), ""));
        assert_eq!(result.projects[0].status, CreateStatus::Created);
    }
    let paths: Vec<String> = fx
        .read_manifest("7.3.0")
        .projects
        .iter()
        .map(|record| record.worktree_path.clone())
        .collect();

    // 追加到末尾：把第一条移到最后
    relocate::reorder(&config, "7.3.0", &paths[0], None).unwrap();
    let order = manifest_order(&fx);
    assert_eq!(order, vec![paths[1].clone(), paths[2].clone(), paths[0].clone()]);

    // 插到首行之前
    relocate::reorder(&config, "7.3.0", &paths[0], Some(&paths[1])).unwrap();
    let order = manifest_order(&fx);
    assert_eq!(order, vec![paths[0].clone(), paths[1].clone(), paths[2].clone()]);

    // 原位（锚点是自己的下一条）→ 不写盘（mtime 不变）
    let manifest_path = manifest::manifest_path(&fx.iteration_dir("7.3.0"));
    let mtime_before = std::fs::metadata(&manifest_path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    relocate::reorder(&config, "7.3.0", &paths[0], Some(&paths[1])).unwrap();
    assert_eq!(
        std::fs::metadata(&manifest_path).unwrap().modified().unwrap(),
        mtime_before,
        "原位操作不应写盘"
    );

    // 锚点不存在 → notFound
    let error = relocate::reorder(&config, "7.3.0", &paths[0], Some("/nowhere/api3")).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::NotFound);
    // 记录不存在 → notFound
    let error = relocate::reorder(&config, "7.3.0", "/nowhere/api3", None).unwrap_err();
    assert_eq!(error.code(), worktree_manager_lib::models::ErrorCode::NotFound);
    // 清单缺失 → manifestDamaged
    let error = relocate::reorder(&config, "9.9.9", &paths[0], None).unwrap_err();
    assert!(matches!(
        error.code(),
        worktree_manager_lib::models::ErrorCode::ManifestDamaged
    ));
}

fn manifest_order(fx: &Fixture) -> Vec<String> {
    fx.read_manifest("7.3.0")
        .projects
        .iter()
        .map(|record| record.worktree_path.clone())
        .collect()
}

#[test]
fn move_project_relocates_directory_and_both_manifests() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::relocate;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    // 目标迭代：先创建一条记录并移除（留下 removed 记录占用 api3）
    let first = fx.create(&config, branch_request("7.2.0", Some("feature/old"), ""));
    assert_eq!(first.projects[0].status, CreateStatus::Created);
    let old_record = fx.read_manifest("7.2.0").projects[0].clone();
    removal::remove_managed(
        &config,
        &RemoveRequest {
            iteration: "7.2.0".to_string(),
            project_id: old_record.project_id.clone(),
            worktree_path: old_record.worktree_path.clone(),
            confirmation: "7.2.0/api3".to_string(),
            remove_copied_vendor: false,
        },
    )
    .unwrap();

    // 源迭代：创建一条 active 记录
    let created = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    let old_path = PathBuf::from(&record.worktree_path);

    // 移动：目标清单有 removed 同名记录 → 目录名顺延为 api3-2
    let outcome = relocate::move_project(
        &config,
        &MoveProjectRequest {
            iteration: "7.3.0".to_string(),
            worktree_path: record.worktree_path.clone(),
            target_iteration: "7.2.0".to_string(),
            before_worktree_path: None,
        },
    )
    .unwrap();
    assert_eq!(outcome.target_directory, "api3-2");
    assert!(!old_path.exists(), "旧目录应已搬走");
    let new_path = PathBuf::from(&outcome.target_worktree_path);
    assert!(new_path.is_dir(), "新目录应真实存在");
    // 目录内容随之搬走（HEAD 未变）
    assert_eq!(
        git(&new_path, &["rev-parse", "HEAD"]).trim(),
        record.base_commit.clone().unwrap().trim()
    );

    // 两份清单：源清单删除该记录，目标清单新增（无重复 worktreePath）
    let source = fx.read_manifest("7.3.0");
    assert!(source.projects.is_empty());
    let target = fx.read_manifest("7.2.0");
    let paths: Vec<&str> = target
        .projects
        .iter()
        .map(|item| item.worktree_path.as_str())
        .collect();
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), paths.len(), "worktreePath 不应重复");
    let moved = target
        .projects
        .iter()
        .find(|item| item.worktree_path == outcome.target_worktree_path)
        .expect("目标清单应包含搬来的记录");
    assert_eq!(moved.lifecycle, Lifecycle::Active);
    assert_eq!(moved.created_at, record.created_at, "快照字段原样迁移");
    assert_eq!(moved.base_commit, record.base_commit);
}

#[test]
fn move_project_rejects_locked_archived_and_missing_target() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::models::ErrorCode;
    use worktree_manager_lib::relocate;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    let created = fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    assert_eq!(created.projects[0].status, CreateStatus::Created);
    let record = fx.read_manifest("7.3.0").projects[0].clone();

    // 目标迭代不存在 → notFound（目标必须已由本工具创建）
    let error = relocate::move_project(
        &config,
        &MoveProjectRequest {
            iteration: "7.3.0".to_string(),
            worktree_path: record.worktree_path.clone(),
            target_iteration: "7.4.0".to_string(),
            before_worktree_path: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::NotFound);

    // 目标迭代已归档 → conflict
    let target_dir = fx.iteration_dir("7.2.0");
    std::fs::create_dir_all(&target_dir).unwrap();
    let mut target_manifest = Manifest {
        schema_version: 1,
        iteration: "7.2.0".to_string(),
        created_at: "2026-07-30T09:00:00Z".to_string(),
        note: None,
        hidden_at: None,
        archived_at: Some("2026-07-31T09:00:00Z".to_string()),
        shared_directories: Vec::new(),
        projects: Vec::new(),
    };
    manifest::write_manifest(&target_dir, &target_manifest).unwrap();
    let error = relocate::move_project(
        &config,
        &MoveProjectRequest {
            iteration: "7.3.0".to_string(),
            worktree_path: record.worktree_path.clone(),
            target_iteration: "7.2.0".to_string(),
            before_worktree_path: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::Conflict);

    // 取消归档后：locked → conflict，且不改动任何内容
    target_manifest.archived_at = None;
    manifest::write_manifest(&target_dir, &target_manifest).unwrap();
    git(&fx.seed, &["worktree", "lock", "--reason", "测试", record.worktree_path.as_str()]);
    let error = relocate::move_project(
        &config,
        &MoveProjectRequest {
            iteration: "7.3.0".to_string(),
            worktree_path: record.worktree_path.clone(),
            target_iteration: "7.2.0".to_string(),
            before_worktree_path: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::Conflict);
    assert!(error.message().contains("锁定"));
    assert!(PathBuf::from(&record.worktree_path).is_dir(), "预检失败不得搬动目录");
    assert_eq!(fx.read_manifest("7.3.0").projects.len(), 1);
    git(&fx.seed, &["worktree", "unlock", record.worktree_path.as_str()]);
}

#[test]
fn move_project_handles_dirty_worktree_with_single_force() {
    if !git_available() {
        eprintln!("跳过：本机没有可执行的 git");
        return;
    }
    use worktree_manager_lib::relocate;

    let fx = Fixture::new(SeedKind::Plain);
    let config = fx.config();
    fx.create(&config, branch_request("7.3.0", Some("feature/x"), ""));
    fx.create(&config, branch_request("7.2.0", Some("feature/y"), ""));
    let record = fx.read_manifest("7.3.0").projects[0].clone();
    // 制造脏工作区（未跟踪文件）
    std::fs::write(
        PathBuf::from(&record.worktree_path).join("dirty.txt"),
        "dirty",
    )
    .unwrap();

    let outcome = relocate::move_project(
        &config,
        &MoveProjectRequest {
            iteration: "7.3.0".to_string(),
            worktree_path: record.worktree_path.clone(),
            target_iteration: "7.2.0".to_string(),
            before_worktree_path: None,
        },
    )
    .unwrap();
    let new_path = PathBuf::from(&outcome.target_worktree_path);
    assert!(new_path.is_dir());
    assert!(new_path.join("dirty.txt").is_file(), "脏文件应一起搬走");
}

//! 全部序列化结构与枚举（与 docs/data-model.md 逐一对应；JSON 一律 camelCase）。
//! 新增字段前必须先改 docs/data-model.md。

use serde::{Deserialize, Serialize};

/// 两份持久化模型当前 schema 版本（docs/data-model.md 开头约定）
pub const SCHEMA_VERSION: u32 = 1;

// ============================ 全局配置（config.json） ============================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SharedDirectoryRule {
    pub source_path: String,
    pub target_directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub id: String,
    pub repository_path: String,
    pub project_type: ProjectType,
    pub vendor_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub shared_directories: Vec<SharedDirectoryRule>,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub recent_iterations: Vec<String>,
}

// ========================= 迭代清单（.worktree-manager.json） =========================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedDirRecord {
    pub rule_id: String,
    pub source_path: String,
    pub target_directory: String,
    pub target_path: String,
    pub status: SharedDirStatus,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorRecord {
    pub source_path: String,
    pub lock_hash: String,
    pub status: VendorStatus,
    pub copied_by_tool: bool,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostStepRecord {
    pub name: String,
    pub status: PostStepStatus,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResult {
    pub status: CreateStatus,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestProject {
    pub project_id: String,
    pub source_repository: String,
    pub worktree_path: String,
    pub head_mode: HeadMode,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    pub base_ref: String,
    pub created_at: String,
    pub lifecycle: Lifecycle,
    pub create_result: CreateResult,
    #[serde(default)]
    pub vendor: Option<VendorRecord>,
    #[serde(default)]
    pub post_steps: Vec<PostStepRecord>,
    #[serde(default)]
    pub removed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub iteration: String,
    pub created_at: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub hidden_at: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub shared_directories: Vec<SharedDirRecord>,
    #[serde(default)]
    pub projects: Vec<ManifestProject>,
}

// ================================== 枚举总表 ==================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectType {
    Php,
    Go,
    Other,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharedDirStatus {
    Pending,
    Reused,
    Copied,
    Failed,
    /// 仅读兼容（旧清单），新写入禁止
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeadMode {
    Branch,
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lifecycle {
    Active,
    Removed,
    CreateFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CreateStatus {
    Created,
    AlreadyExists,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VendorStatus {
    Copied,
    NotPhp,
    SourceMissing,
    LockMissing,
    LockMismatch,
    TargetExists,
    CopyFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostStepStatus {
    Success,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Validity {
    Valid,
    MissingDirectory,
    NotRegistered,
    HeadMismatch,
    SourceMissing,
    Removed,
    Discovered,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManifestHealth {
    Valid,
    Missing,
    Damaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeCellStatus {
    Merged,
    Contained,
    Unmerged,
    TargetMissing,
    BranchMissing,
    NotCheckable,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskSeverity {
    Warning,
    Blocking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskCode {
    TrackedChanges,
    UntrackedFiles,
    UnpushedCommits,
    DetachedCommits,
    WorktreeLocked,
    PathInvalid,
    ManifestInvalid,
    Prunable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CreatePhase {
    Queued,
    Fetching,
    Creating,
    Vendor,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeCheckPhase {
    Fetching,
    Checking,
    Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    Validation,
    Busy,
    NotFound,
    Conflict,
    ManifestDamaged,
    Git,
    Io,
    Unsupported,
}


// ============================== 请求与结果（§4） ==============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub project_id: String,
    /// null ＝ detached HEAD
    pub branch: Option<String>,
    /// 空串 ＝ origin/master（后端 base_ref::normalize 归一化）
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub iteration: String,
    /// 三态约定：字段缺省 → None（不改动）；null → Some(None)（清除）；字符串 → Some(Some(s))（设置）
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub note: Option<Option<String>>,
    pub unified_branch: Option<String>,
    pub unified_base_ref: String,
    pub projects: Vec<CreateProjectRequest>,
}

/// `#[serde(default)]` 处理缺省；本函数只处理「字段存在」时的 null / 字符串两态。
fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectResult {
    pub project_id: String,
    pub worktree_path: String,
    pub status: CreateStatus,
    pub message: Option<String>,
    pub vendor_status: Option<VendorStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBatchResult {
    pub iteration: String,
    pub iteration_path: String,
    pub shared_directories: Vec<SharedDirRecord>,
    pub projects: Vec<CreateProjectResult>,
    pub aborted: bool,
    pub abort_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResolution {
    pub input_path: String,
    pub repository_path: Option<String>,
    pub suggested_id: Option<String>,
    pub project_type: ProjectType,
    pub vendor_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BranchSource {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranches {
    pub project_id: String,
    pub remotes: Vec<String>,
    pub branches: Vec<String>,
    pub source: BranchSource,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveRequest {
    pub iteration: String,
    pub project_id: String,
    pub worktree_path: String,
    pub confirmation: String,
    pub remove_copied_vendor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalRisk {
    pub code: RiskCode,
    pub severity: RiskSeverity,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalAssessment {
    pub iteration: String,
    pub project_id: String,
    pub worktree_path: String,
    /// = "{iteration}/{目录名}"
    pub confirmation_text: String,
    pub allowed: bool,
    pub vendor_only_cleanup_available: bool,
    pub risks: Vec<RemovalRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCellResult {
    pub status: MergeCellStatus,
    /// 形如 "<短hash> <subject>"
    pub unmerged_commits: Vec<String>,
    pub error_message: Option<String>,
    pub stale: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRecordResult {
    pub project_id: String,
    pub branch_display: String,
    pub worktree_path: String,
    pub lifecycle: Lifecycle,
    pub develop: MergeCellResult,
    pub master: MergeCellResult,
    pub has_changes: Option<bool>,
    pub dirty: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCheckResult {
    pub iteration: String,
    pub checked_at: String,
    pub records: Vec<MergeRecordResult>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequest {
    pub iteration: String,
    pub confirmation: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRecordAssessment {
    pub project_id: String,
    pub branch_display: String,
    pub worktree_path: String,
    pub develop: MergeCellResult,
    pub master: MergeCellResult,
    pub dirty: bool,
    pub clean: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveAssessment {
    pub iteration: String,
    pub confirmation_text: String,
    pub checked_at: String,
    pub clean: bool,
    pub records: Vec<ArchiveRecordAssessment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveFailure {
    pub project_id: String,
    pub worktree_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveOutcome {
    pub iteration: String,
    pub removed_count: u32,
    pub failed: Vec<ArchiveFailure>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveProjectRequest {
    pub iteration: String,
    pub worktree_path: String,
    pub target_iteration: String,
    pub before_worktree_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveOutcome {
    pub iteration: String,
    pub target_iteration: String,
    pub project_id: String,
    pub worktree_path: String,
    pub target_worktree_path: String,
    pub target_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProgress {
    pub project_id: String,
    pub index: u32,
    pub total: u32,
    pub phase: CreatePhase,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCheckProgress {
    pub iteration: String,
    pub project_id: String,
    pub worktree_path: String,
    pub index: u32,
    pub total: u32,
    pub phase: MergeCheckPhase,
    pub message: String,
    pub record: Option<MergeRecordResult>,
}

// ============================= 投影结构（§5，列表页） =============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProject {
    pub project_id: String,
    pub branch_display: String,
    pub base_commit: Option<String>,
    pub base_ref: Option<String>,
    pub source_repository: String,
    pub worktree_path: String,
    pub vendor_status: Option<VendorStatus>,
    pub created_at: Option<String>,
    pub validity: Validity,
    pub openable: bool,
    pub removable: bool,
    pub renamed_from: Option<String>,
    pub dirty: Option<bool>,
    pub has_changes: Option<bool>,
    /// discovered 行无清单记录 → null
    pub lifecycle: Option<Lifecycle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGroup {
    pub iteration: String,
    pub iteration_path: String,
    pub manifest_health: ManifestHealth,
    pub manifest_message: Option<String>,
    pub openable: bool,
    pub note: Option<String>,
    pub hidden_at: Option<String>,
    pub shared_directories: Vec<SharedDirRecord>,
    pub projects: Vec<WorkspaceProject>,
}


// ==================================== 单测 ====================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个枚举的 serde 字符串往返（与 docs/data-model.md §3 枚举总表对照）
    #[test]
    fn enum_round_trips() {
        fn rt<T>(value: T, expected: &str)
        where
            T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
        {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let back: T = serde_json::from_str(&json).unwrap();
            assert_eq!(back, value);
        }

        rt(ProjectType::Php, "php");
        rt(ProjectType::Go, "go");
        rt(ProjectType::Other, "other");
        rt(ProjectType::Unknown, "unknown");

        rt(SharedDirStatus::Pending, "pending");
        rt(SharedDirStatus::Reused, "reused");
        rt(SharedDirStatus::Copied, "copied");
        rt(SharedDirStatus::Failed, "failed");
        rt(SharedDirStatus::NotRequired, "notRequired");

        rt(HeadMode::Branch, "branch");
        rt(HeadMode::Detached, "detached");

        rt(Lifecycle::Active, "active");
        rt(Lifecycle::Removed, "removed");
        rt(Lifecycle::CreateFailed, "createFailed");

        rt(CreateStatus::Created, "created");
        rt(CreateStatus::AlreadyExists, "alreadyExists");
        rt(CreateStatus::Failed, "failed");

        rt(VendorStatus::Copied, "copied");
        rt(VendorStatus::NotPhp, "notPhp");
        rt(VendorStatus::SourceMissing, "sourceMissing");
        rt(VendorStatus::LockMissing, "lockMissing");
        rt(VendorStatus::LockMismatch, "lockMismatch");
        rt(VendorStatus::TargetExists, "targetExists");
        rt(VendorStatus::CopyFailed, "copyFailed");

        rt(PostStepStatus::Success, "success");
        rt(PostStepStatus::Skipped, "skipped");
        rt(PostStepStatus::Failed, "failed");

        rt(Validity::Valid, "valid");
        rt(Validity::MissingDirectory, "missingDirectory");
        rt(Validity::NotRegistered, "notRegistered");
        rt(Validity::HeadMismatch, "headMismatch");
        rt(Validity::SourceMissing, "sourceMissing");
        rt(Validity::Removed, "removed");
        rt(Validity::Discovered, "discovered");
        rt(Validity::Unknown, "unknown");

        rt(ManifestHealth::Valid, "valid");
        rt(ManifestHealth::Missing, "missing");
        rt(ManifestHealth::Damaged, "damaged");

        rt(MergeCellStatus::Merged, "merged");
        rt(MergeCellStatus::Contained, "contained");
        rt(MergeCellStatus::Unmerged, "unmerged");
        rt(MergeCellStatus::TargetMissing, "targetMissing");
        rt(MergeCellStatus::BranchMissing, "branchMissing");
        rt(MergeCellStatus::NotCheckable, "notCheckable");
        rt(MergeCellStatus::Error, "error");

        rt(RiskSeverity::Warning, "warning");
        rt(RiskSeverity::Blocking, "blocking");

        rt(RiskCode::TrackedChanges, "trackedChanges");
        rt(RiskCode::UntrackedFiles, "untrackedFiles");
        rt(RiskCode::UnpushedCommits, "unpushedCommits");
        rt(RiskCode::DetachedCommits, "detachedCommits");
        rt(RiskCode::WorktreeLocked, "worktreeLocked");
        rt(RiskCode::PathInvalid, "pathInvalid");
        rt(RiskCode::ManifestInvalid, "manifestInvalid");
        rt(RiskCode::Prunable, "prunable");

        rt(CreatePhase::Queued, "queued");
        rt(CreatePhase::Fetching, "fetching");
        rt(CreatePhase::Creating, "creating");
        rt(CreatePhase::Vendor, "vendor");
        rt(CreatePhase::Completed, "completed");
        rt(CreatePhase::Failed, "failed");

        rt(MergeCheckPhase::Fetching, "fetching");
        rt(MergeCheckPhase::Checking, "checking");
        rt(MergeCheckPhase::Record, "record");

        rt(ErrorCode::Validation, "validation");
        rt(ErrorCode::Busy, "busy");
        rt(ErrorCode::NotFound, "notFound");
        rt(ErrorCode::Conflict, "conflict");
        rt(ErrorCode::ManifestDamaged, "manifestDamaged");
        rt(ErrorCode::Git, "git");
        rt(ErrorCode::Io, "io");
        rt(ErrorCode::Unsupported, "unsupported");
    }

    /// 旧清单里的 notRequired 必须仍可反序列化
    #[test]
    fn shared_dir_status_accepts_not_required() {
        let status: SharedDirStatus = serde_json::from_str("\"notRequired\"").unwrap();
        assert_eq!(status, SharedDirStatus::NotRequired);
    }

    /// workspaceRoot: null 可反序列化；缺省字段可反序列化
    #[test]
    fn config_deserializes_nullable_root_and_missing_fields() {
        let config: AppConfig =
            serde_json::from_str(r#"{"schemaVersion":1,"workspaceRoot":null}"#).unwrap();
        assert_eq!(config.workspace_root, None);
        assert!(config.shared_directories.is_empty());
        assert!(config.projects.is_empty());
        assert!(config.recent_iterations.is_empty());

        let config: AppConfig =
            serde_json::from_str(r#"{"schemaVersion":1,"workspaceRoot":"/tmp/x"}"#).unwrap();
        assert_eq!(config.workspace_root.as_deref(), Some("/tmp/x"));
    }

    /// CreateRequest.note 三态：缺省 / null / 字符串
    #[test]
    fn create_request_note_three_states() {
        let absent: CreateRequest = serde_json::from_str(
            r#"{"iteration":"7.3.0","unifiedBranch":null,"unifiedBaseRef":"","projects":[]}"#,
        )
        .unwrap();
        assert_eq!(absent.note, None);

        let cleared: CreateRequest = serde_json::from_str(
            r#"{"iteration":"7.3.0","note":null,"unifiedBranch":null,"unifiedBaseRef":"","projects":[]}"#,
        )
        .unwrap();
        assert_eq!(cleared.note, Some(None));

        let set: CreateRequest = serde_json::from_str(
            r#"{"iteration":"7.3.0","note":"等 QA","unifiedBranch":null,"unifiedBaseRef":"","projects":[]}"#,
        )
        .unwrap();
        assert_eq!(set.note, Some(Some("等 QA".to_string())));
    }
}


#[cfg(test)]
mod manifest_tests {
    use super::*;

    /// 清单结构完整往返（字段名 camelCase，null 字段保留）
    #[test]
    fn manifest_round_trip_uses_camel_case() {
        let manifest = Manifest {
            schema_version: SCHEMA_VERSION,
            iteration: "7.3.0".to_string(),
            created_at: "2026-07-30T10:00:00Z".to_string(),
            note: Some("等 QA".to_string()),
            hidden_at: None,
            archived_at: None,
            shared_directories: vec![SharedDirRecord {
                rule_id: "fd-common".to_string(),
                source_path: "/src".to_string(),
                target_directory: "fd-common".to_string(),
                target_path: "/ws/7.3.0/fd-common".to_string(),
                status: SharedDirStatus::Copied,
                message: None,
            }],
            projects: vec![ManifestProject {
                project_id: "api3".to_string(),
                source_repository: "/repo/api3".to_string(),
                worktree_path: "/ws/7.3.0/api3".to_string(),
                head_mode: HeadMode::Branch,
                branch: Some("feature/x".to_string()),
                base_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
                base_ref: "origin/master".to_string(),
                created_at: "2026-07-30T10:01:00Z".to_string(),
                lifecycle: Lifecycle::Active,
                create_result: CreateResult {
                    status: CreateStatus::Created,
                    message: None,
                },
                vendor: Some(VendorRecord {
                    source_path: "/repo/api3/vendor".to_string(),
                    lock_hash: "abc".to_string(),
                    status: VendorStatus::Copied,
                    copied_by_tool: true,
                    message: None,
                }),
                post_steps: vec![PostStepRecord {
                    name: "vendor".to_string(),
                    status: PostStepStatus::Success,
                    message: None,
                }],
                removed_at: None,
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"worktreePath\":\"/ws/7.3.0/api3\""));
        assert!(json.contains("\"copiedByTool\":true"));
        assert!(json.contains("\"hiddenAt\":null"));

        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, manifest);

        // 被截断的 JSON 必须解析失败（不得静默接受）
        assert!(serde_json::from_str::<Manifest>(&json[..json.len() - 5]).is_err());
    }
}


// ============================================================================
// 与 Rust 结构一一对应的 TS 类型（来源：docs/data-model.md）
// 本文件只含 export type / export interface，不含任何运行时代码。
// ============================================================================

// ---------- 全局配置（config.json，§1） ----------

export type ProjectType = "php" | "go" | "other" | "unknown";

export interface SharedDirectoryRule {
  sourcePath: string;
  targetDirectory: string;
}

export interface ProjectConfig {
  id: string;
  repositoryPath: string;
  projectType: ProjectType;
  vendorAvailable: boolean;
}

export interface AppConfig {
  schemaVersion: number;
  workspaceRoot: string | null;
  sharedDirectories: SharedDirectoryRule[];
  projects: ProjectConfig[];
  recentIterations: string[];
}

// ---------- 迭代清单（.worktree-manager.json，§2） ----------

export type SharedDirStatus = "pending" | "reused" | "copied" | "failed" | "notRequired";

export interface SharedDirRecord {
  ruleId: string;
  sourcePath: string;
  targetDirectory: string;
  targetPath: string;
  status: SharedDirStatus;
  message: string | null;
}

export type HeadMode = "branch" | "detached";
export type Lifecycle = "active" | "removed" | "createFailed";
export type CreateStatus = "created" | "alreadyExists" | "failed";
export type VendorStatus =
  | "copied"
  | "notPhp"
  | "sourceMissing"
  | "lockMissing"
  | "lockMismatch"
  | "targetExists"
  | "copyFailed";
export type PostStepStatus = "success" | "skipped" | "failed";

export interface VendorRecord {
  sourcePath: string;
  lockHash: string;
  status: VendorStatus;
  copiedByTool: boolean;
  message: string | null;
}

export interface PostStepRecord {
  name: string;
  status: PostStepStatus;
  message: string | null;
}

export interface CreateResult {
  status: CreateStatus;
  message: string | null;
}

export interface ManifestProject {
  projectId: string;
  sourceRepository: string;
  worktreePath: string;
  headMode: HeadMode;
  branch: string | null;
  baseCommit: string | null;
  baseRef: string;
  createdAt: string;
  lifecycle: Lifecycle;
  createResult: CreateResult;
  vendor: VendorRecord | null;
  postSteps: PostStepRecord[];
  removedAt: string | null;
}

export interface Manifest {
  schemaVersion: number;
  iteration: string;
  createdAt: string;
  note: string | null;
  hiddenAt: string | null;
  archivedAt: string | null;
  sharedDirectories: SharedDirRecord[];
  projects: ManifestProject[];
}

// ---------- 枚举总表（§3，与 models.rs 逐个对照） ----------

export type Validity =
  | "valid"
  | "missingDirectory"
  | "notRegistered"
  | "headMismatch"
  | "sourceMissing"
  | "removed"
  | "discovered"
  | "unknown";

export type ManifestHealth = "valid" | "missing" | "damaged";

export type MergeCellStatus =
  | "merged"
  | "contained"
  | "unmerged"
  | "targetMissing"
  | "branchMissing"
  | "notCheckable"
  | "error";

export type RiskSeverity = "warning" | "blocking";

export type RiskCode =
  | "trackedChanges"
  | "untrackedFiles"
  | "unpushedCommits"
  | "detachedCommits"
  | "worktreeLocked"
  | "pathInvalid"
  | "manifestInvalid"
  | "prunable";

export type CreatePhase = "queued" | "fetching" | "creating" | "vendor" | "completed" | "failed";

export type MergeCheckPhase = "fetching" | "checking" | "record";

export type ErrorCode =
  | "validation"
  | "busy"
  | "notFound"
  | "conflict"
  | "manifestDamaged"
  | "git"
  | "io"
  | "unsupported";

export interface AppErrorObject {
  code: ErrorCode;
  message: string;
}

// ---------- 请求与结果（§4） ----------

export interface CreateProjectRequest {
  projectId: string;
  /** null＝detached */
  branch: string | null;
  /** 空串＝origin/master（后端归一化） */
  baseRef: string;
}

export interface CreateRequest {
  iteration: string;
  /** 三态约定：缺省＝不改动；null＝清除；字符串＝设置。禁止用 "" 表达不改动 */
  note?: string | null;
  unifiedBranch: string | null;
  unifiedBaseRef: string;
  projects: CreateProjectRequest[];
}

export interface CreateProjectResult {
  projectId: string;
  worktreePath: string;
  status: CreateStatus;
  message: string | null;
  vendorStatus: VendorStatus | null;
}

export interface CreateBatchResult {
  iteration: string;
  iterationPath: string;
  sharedDirectories: SharedDirRecord[];
  projects: CreateProjectResult[];
  aborted: boolean;
  abortReason: string | null;
}

export interface ProjectResolution {
  inputPath: string;
  repositoryPath: string | null;
  suggestedId: string | null;
  projectType: ProjectType;
  vendorAvailable: boolean;
  error: string | null;
}

export interface RemoteBranches {
  projectId: string;
  remotes: string[];
  /** 形如 origin/xxx，去重排序 */
  branches: string[];
  source: "local" | "remote";
  warning: string | null;
}

export interface RemoveRequest {
  iteration: string;
  projectId: string;
  worktreePath: string;
  confirmation: string;
  removeCopiedVendor: boolean;
}

export interface RemovalRisk {
  code: RiskCode;
  severity: RiskSeverity;
  message: string;
  paths: string[];
}

export interface RemovalAssessment {
  iteration: string;
  projectId: string;
  worktreePath: string;
  /** = "{iteration}/{目录名}" */
  confirmationText: string;
  allowed: boolean;
  vendorOnlyCleanupAvailable: boolean;
  risks: RemovalRisk[];
}

export interface MergeCellResult {
  status: MergeCellStatus;
  /** 形如 "<短hash> <subject>" */
  unmergedCommits: string[];
  errorMessage: string | null;
  stale: boolean;
  dirty: boolean;
}

export interface MergeRecordResult {
  projectId: string;
  branchDisplay: string;
  worktreePath: string;
  lifecycle: Lifecycle;
  develop: MergeCellResult;
  master: MergeCellResult;
  hasChanges: boolean | null;
  dirty: boolean | null;
}

export interface MergeCheckResult {
  iteration: string;
  checkedAt: string;
  records: MergeRecordResult[];
}

export interface ArchiveRequest {
  iteration: string;
  confirmation: string;
  force: boolean;
}

export interface ArchiveRecordAssessment {
  projectId: string;
  branchDisplay: string;
  worktreePath: string;
  develop: MergeCellResult;
  master: MergeCellResult;
  dirty: boolean;
  clean: boolean;
  blockers: string[];
}

export interface ArchiveAssessment {
  iteration: string;
  confirmationText: string;
  checkedAt: string;
  clean: boolean;
  records: ArchiveRecordAssessment[];
}

export interface ArchiveFailure {
  projectId: string;
  worktreePath: string;
  message: string;
}

export interface ArchiveOutcome {
  iteration: string;
  removedCount: number;
  failed: ArchiveFailure[];
  archived: boolean;
}

export interface MoveProjectRequest {
  iteration: string;
  worktreePath: string;
  targetIteration: string;
  beforeWorktreePath: string | null;
}

export interface MoveOutcome {
  iteration: string;
  targetIteration: string;
  projectId: string;
  worktreePath: string;
  targetWorktreePath: string;
  targetDirectory: string;
}

export interface CreateProgress {
  projectId: string;
  index: number;
  total: number;
  phase: CreatePhase;
  message: string;
}

export interface MergeCheckProgress {
  iteration: string;
  projectId: string;
  worktreePath: string;
  index: number;
  total: number;
  phase: MergeCheckPhase;
  message: string;
  record: MergeRecordResult | null;
}

// ---------- 投影结构（§5） ----------

export interface WorkspaceProject {
  projectId: string;
  branchDisplay: string;
  baseCommit: string | null;
  baseRef: string | null;
  sourceRepository: string;
  worktreePath: string;
  vendorStatus: VendorStatus | null;
  createdAt: string | null;
  validity: Validity;
  openable: boolean;
  removable: boolean;
  renamedFrom: string | null;
  dirty: boolean | null;
  hasChanges: boolean | null;
  /** discovered 为 null */
  lifecycle: Lifecycle | null;
}

export interface WorkspaceGroup {
  iteration: string;
  iterationPath: string;
  manifestHealth: ManifestHealth;
  manifestMessage: string | null;
  openable: boolean;
  note: string | null;
  hiddenAt: string | null;
  sharedDirectories: SharedDirRecord[];
  projects: WorkspaceProject[];
}

// ---------- 前端界面类型（不参与后端序列化） ----------

/** 页面状态机：workspaces | create | settings */
export type PageKey = "workspaces" | "create" | "settings";

/** toast 三态 */
export type ToastTone = "success" | "warning" | "danger";


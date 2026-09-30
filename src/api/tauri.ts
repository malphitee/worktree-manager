// 全部 invoke 与 listen 的唯一出口（architecture.md §7.1 / §7.3）。
// 浏览器演示模式（无 __TAURI_INTERNALS__）：读命令返回 demo-data.ts 的假数据；
// 写命令与长任务 reject { code: "unsupported" }；事件订阅返回空 unlisten。
// 注意：@tauri-apps/api 与 @tauri-apps/plugin-dialog 必须在函数体内动态 import。
import type {
  AppConfig,
  AppErrorObject,
  ArchiveAssessment,
  ArchiveOutcome,
  ArchiveRequest,
  CreateBatchResult,
  CreateProgress,
  CreateRequest,
  MergeCheckProgress,
  MergeCheckResult,
  MoveOutcome,
  MoveProjectRequest,
  ProjectResolution,
  RemoteBranches,
  RemoveRequest,
  RemovalAssessment,
  WorkspaceGroup,
} from "../types";
import * as demo from "./demo-data";

const DEMO_UNSUPPORTED: AppErrorObject = {
  code: "unsupported",
  message: "浏览器演示模式不支持此操作",
};

/** 是否为真实 Tauri 环境（否则为浏览器演示模式） */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function unsupported<T>(): Promise<T> {
  return Promise.reject({ ...DEMO_UNSUPPORTED });
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

/** 把任意 invoke 失败统一包装为 { code, message }（workflows.md §12.6） */
export function toAppError(err: unknown): AppErrorObject {
  if (err !== null && typeof err === "object" && "code" in err && "message" in err) {
    const candidate = err as { code: unknown; message: unknown };
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code as AppErrorObject["code"], message: candidate.message };
    }
  }
  return { code: "io", message: String(err) };
}

// ---------- 配置 ----------

export function getConfig(): Promise<AppConfig> {
  return isTauri() ? invokeCommand("get_config") : Promise.resolve(demo.demoConfig);
}

export function saveConfig(config: AppConfig): Promise<AppConfig> {
  return isTauri() ? invokeCommand("save_config", { config }) : unsupported();
}

export function resolveProjects(paths: string[]): Promise<ProjectResolution[]> {
  return isTauri() ? invokeCommand("resolve_projects", { paths }) : Promise.resolve(demo.demoProjectResolutions);
}

// ---------- 工作区列表 ----------

export function listWorkspaces(reconcile: boolean): Promise<WorkspaceGroup[]> {
  return isTauri()
    ? invokeCommand("list_workspaces", { reconcile })
    : Promise.resolve(demo.demoWorkspaceGroups);
}

// ---------- 创建 ----------

export function createWorkspaces(request: CreateRequest): Promise<CreateBatchResult> {
  return isTauri() ? invokeCommand("create_workspaces", { request }) : unsupported();
}

// ---------- 基分支（013） ----------

export function listRemoteBranches(projectId: string, includeRemote: boolean): Promise<RemoteBranches> {
  return isTauri()
    ? invokeCommand("list_remote_branches", { projectId, includeRemote })
    : Promise.resolve(demo.demoRemoteBranches);
}

// ---------- 合并检查（001 / 003） ----------

export function checkMergeStatus(
  iteration: string,
  projectId?: string,
  worktreePath?: string,
): Promise<MergeCheckResult> {
  if (!isTauri()) {
    return unsupported();
  }
  const args: Record<string, unknown> = { iteration };
  if (projectId !== undefined) {
    args.projectId = projectId;
  }
  if (worktreePath !== undefined) {
    args.worktreePath = worktreePath;
  }
  return invokeCommand("check_merge_status", args);
}

// ---------- 移除 ----------

export function assessRemoval(
  iteration: string,
  projectId: string,
  worktreePath: string,
): Promise<RemovalAssessment> {
  return isTauri()
    ? invokeCommand("assess_removal", { iteration, projectId, worktreePath })
    : Promise.resolve(demo.demoRemovalAssessment);
}

export function removeWorktree(request: RemoveRequest): Promise<null> {
  return isTauri() ? invokeCommand("remove_worktree", { request }) : unsupported();
}

export function assessDiscoveredRemoval(sourcePath: string): Promise<RemovalAssessment> {
  return isTauri()
    ? invokeCommand("assess_discovered_removal", { sourcePath })
    : Promise.resolve(demo.demoDiscoveredRemovalAssessment);
}

export function removeDiscoveredWorktree(params: {
  iteration: string;
  projectId: string;
  worktreePath: string;
  sourcePath: string;
  confirmation: string;
  removeCopiedVendor: boolean;
}): Promise<null> {
  return isTauri() ? invokeCommand("remove_discovered_worktree", params) : unsupported();
}

// ---------- 归档（008） ----------

export function assessArchive(iteration: string): Promise<ArchiveAssessment> {
  return isTauri() ? invokeCommand("assess_archive", { iteration }) : unsupported();
}

export function archiveIteration(request: ArchiveRequest): Promise<ArchiveOutcome> {
  return isTauri() ? invokeCommand("archive_iteration", { request }) : unsupported();
}

// ---------- 备注 / 隐藏（009 / 010） ----------

export function setIterationNote(iteration: string, note: string | null): Promise<string | null> {
  return isTauri() ? invokeCommand("set_iteration_note", { iteration, note }) : unsupported();
}

export function setIterationHidden(iteration: string, hidden: boolean): Promise<string | null> {
  return isTauri() ? invokeCommand("set_iteration_hidden", { iteration, hidden }) : unsupported();
}

// ---------- 打开目录 ----------

export function openIteration(iteration: string): Promise<null> {
  return isTauri() ? invokeCommand("open_iteration", { iteration }) : unsupported();
}

export function openProject(iteration: string, projectId: string, worktreePath: string): Promise<null> {
  return isTauri()
    ? invokeCommand("open_project", { iteration, projectId, worktreePath })
    : unsupported();
}

// ---------- 排序 / 跨迭代移动（014） ----------

export function reorderProject(
  iteration: string,
  worktreePath: string,
  beforeWorktreePath: string | null,
): Promise<null> {
  return isTauri()
    ? invokeCommand("reorder_project", { iteration, worktreePath, beforeWorktreePath })
    : unsupported();
}

export function moveProject(request: MoveProjectRequest): Promise<MoveOutcome> {
  return isTauri() ? invokeCommand("move_project", { request }) : unsupported();
}

// ---------- 事件订阅（3 个；演示模式返回空 unlisten） ----------

export async function onCreateProgress(handler: (progress: CreateProgress) => void): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<CreateProgress>("create-progress", (event) => handler(event.payload));
}

export async function onMergeCheckProgress(
  handler: (progress: MergeCheckProgress) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<MergeCheckProgress>("merge-check-progress", (event) => handler(event.payload));
}

export async function onArchiveProgress(
  handler: (progress: MergeCheckProgress) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<MergeCheckProgress>("archive-progress", (event) => handler(event.payload));
}

// ---------- 目录选择（设置页；不触发任何后端命令） ----------

/** 打开系统目录选择对话框；返回所选绝对路径数组（取消时为空数组） */
export async function pickDirectories(multiple: boolean): Promise<string[]> {
  if (!isTauri()) {
    return [];
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple });
  if (selected === null) {
    return [];
  }
  return Array.isArray(selected) ? selected : [selected];
}


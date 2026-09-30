<script setup lang="ts">
// 应用壳（architecture.md §8）：页面状态机（workspaces | create | settings）、侧栏、顶部横幅、toast、模态层挂载点。
import { computed, onMounted, provide, ref } from "vue";
import * as api from "./api/tauri";
import AppSidebar from "./components/AppSidebar.vue";
import ArchiveDialog from "./components/ArchiveDialog.vue";
import CreateWorkspace from "./components/CreateWorkspace.vue";
import MergeDetailDialog from "./components/MergeDetailDialog.vue";
import OperationView from "./components/OperationView.vue";
import RemovalDialog from "./components/RemovalDialog.vue";
import SettingsView from "./components/SettingsView.vue";
import ToastStack from "./components/ToastStack.vue";
import WorkspaceList from "./components/WorkspaceList.vue";
import type {
  AppConfig,
  ArchiveAssessment,
  CreateBatchResult,
  CreateProgress,
  CreateRequest,
  MergeCellResult,
  PageKey,
  RemovalAssessment,
  ToastTone,
  WorkspaceGroup,
  WorkspaceProject,
} from "./types";
import type { MergeTarget } from "./utils/merge";
import { UI_CONTEXT_KEY } from "./utils/context";
import type { BannerTone } from "./utils/context";
import { countRenamed } from "./utils/status";

interface ToastEntry {
  id: number;
  tone: ToastTone;
  message: string;
}

const EMPTY_CONFIG: AppConfig = {
  schemaVersion: 1,
  workspaceRoot: null,
  sharedDirectories: [],
  projects: [],
  recentIterations: [],
};

const page = ref<PageKey>("workspaces");
const config = ref<AppConfig | null>(null);
const groups = ref<WorkspaceGroup[]>([]);
const reconcileLoading = ref(false);
const banner = ref<{ tone: BannerTone; message: string } | null>(null);
const toasts = ref<ToastEntry[]>([]);
let toastSeq = 0;

// ---------------- 模态状态 ----------------

const operationState = ref<{
  items: CreateProgress[];
  running: boolean;
  result: CreateBatchResult | null;
  errorMessage: string | null;
} | null>(null);

const removalState = ref<{
  iteration: string;
  project: WorkspaceProject;
  assessment: RemovalAssessment;
} | null>(null);

const archiveState = ref<{ iteration: string; assessment: ArchiveAssessment } | null>(null);
const mergeDetailState = ref<{
  iteration: string;
  project: WorkspaceProject;
  target: MergeTarget;
  cell: MergeCellResult;
} | null>(null);

/** 列表组件实例：用于在写操作成功后清除该迭代的内存合并结论 */
const workspaceList = ref<InstanceType<typeof WorkspaceList> | null>(null);

const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
if (!isTauriEnv) {
  banner.value = { tone: "demo", message: "浏览器演示模式：数据为只读假数据" };
}

const emptyMessage = computed(() =>
  config.value?.workspaceRoot
    ? "还没有任何迭代；点击「新建工作区」开始。"
    : "请先在设置中填写工作区根目录",
);

// ---------------- toast / 横幅 ----------------

function toast(message: string, tone: ToastTone = "success"): void {
  toastSeq += 1;
  toasts.value = [...toasts.value, { id: toastSeq, tone, message }];
}

function dismissToast(id: number): void {
  toasts.value = toasts.value.filter((item) => item.id !== id);
}

function setBanner(message: string | null, tone: BannerTone = "error"): void {
  banner.value = message === null ? null : { tone, message };
}

provide(UI_CONTEXT_KEY, { toast, setBanner });

/** 页面级错误进横幅、操作级错误进 toast（architecture.md §8） */
function handleError(err: unknown, level: "toast" | "banner" = "toast"): void {
  const error = api.toAppError(err);
  if (level === "banner") {
    setBanner(error.message, "error");
    return;
  }
  toast(error.message, error.code === "busy" ? "warning" : "danger");
}

function navigate(next: PageKey): void {
  page.value = next;
}

// ---------------- 加载 ----------------

async function loadConfig(): Promise<void> {
  try {
    config.value = await api.getConfig();
  } catch (err) {
    handleError(err, "banner");
  }
}

async function loadGroups(reconcile: boolean): Promise<void> {
  try {
    groups.value = await api.listWorkspaces(reconcile);
  } catch (err) {
    handleError(err, "banner");
  }
}

onMounted(async () => {
  await loadConfig();
  await loadGroups(false);
});

// ---------------- 设置页 ----------------

async function saveConfigValue(next: AppConfig): Promise<void> {
  try {
    config.value = await api.saveConfig(next);
    toast("设置已保存");
    await loadGroups(false);
  } catch (err) {
    handleError(err);
  }
}

// ---------------- 列表 ----------------

async function reconcile(): Promise<void> {
  reconcileLoading.value = true;
  try {
    const next = await api.listWorkspaces(true);
    groups.value = next;
    // 011：复核可能同步了分支改名，N > 0 时汇总提示
    const renamed = countRenamed(next);
    if (renamed > 0) {
      toast(`已同步 ${renamed} 个分支重命名`);
    }
  } catch (err) {
    handleError(err);
  } finally {
    reconcileLoading.value = false;
  }
}

async function openIteration(iteration: string): Promise<void> {
  try {
    await api.openIteration(iteration);
  } catch (err) {
    handleError(err);
  }
}

async function openProject(payload: { iteration: string; project: WorkspaceProject }): Promise<void> {
  try {
    await api.openProject(payload.iteration, payload.project.projectId, payload.project.worktreePath);
  } catch (err) {
    handleError(err);
  }
}

// ---------------- 创建 ----------------

async function startCreate(request: CreateRequest): Promise<void> {
  operationState.value = { items: [], running: true, result: null, errorMessage: null };
  page.value = "create";
  let unlisten: (() => void) | null = null;
  try {
    // 先订阅再 invoke，避免漏掉首个事件
    unlisten = await api.onCreateProgress((progress) => {
      const state = operationState.value;
      if (!state) {
        return;
      }
      const items = [...state.items];
      const index = items.findIndex((item) => item.projectId === progress.projectId);
      if (index >= 0) {
        items[index] = progress;
      } else {
        items.push(progress);
      }
      operationState.value = { ...state, items };
    });
    const result = await api.createWorkspaces(request);
    const state = operationState.value;
    operationState.value = {
      items: state?.items ?? [],
      running: false,
      result,
      errorMessage: state?.errorMessage ?? null,
    };
    if (result.aborted) {
      toast(result.abortReason ?? "创建已终止", "warning");
    } else {
      const created = result.projects.filter((project) => project.status === "created").length;
      toast(`创建完成：${created} 个已创建`);
    }
    await loadConfig();
    await loadGroups(false);
  } catch (err) {
    const error = api.toAppError(err);
    const state = operationState.value;
    operationState.value = {
      items: state?.items ?? [],
      running: false,
      result: null,
      errorMessage: error.message,
    };
    handleError(err);
  } finally {
    // 组件卸载前主动取消订阅
    if (unlisten) {
      unlisten();
    }
  }
}

function backToWorkspaces(): void {
  operationState.value = null;
  page.value = "workspaces";
}

// ---------------- 归档 / 备注 / 隐藏 ----------------

/** 正在评估归档的迭代（卡片「归档」按钮 loading） */
const archiveAssessing = ref<string | null>(null);
const archiveProgress = ref<string | null>(null);
let archiveUnlisten: (() => void) | null = null;

async function requestArchive(iteration: string): Promise<void> {
  if (archiveAssessing.value !== null) {
    return;
  }
  archiveAssessing.value = iteration;
  archiveProgress.value = null;
  try {
    archiveUnlisten = await api.onArchiveProgress((progress) => {
      if (progress.iteration === iteration) {
        archiveProgress.value = progress.message;
      }
    });
    const assessment = await api.assessArchive(iteration);
    archiveState.value = { iteration, assessment };
  } catch (err) {
    handleError(err);
  } finally {
    archiveUnlisten?.();
    archiveUnlisten = null;
    archiveAssessing.value = null;
  }
}

async function confirmArchive(payload: { force: boolean; confirmation: string }): Promise<void> {
  const state = archiveState.value;
  if (!state) {
    return;
  }
  const { iteration } = state;
  archiveProgress.value = null;
  try {
    archiveUnlisten = await api.onArchiveProgress((progress) => {
      if (progress.iteration === iteration) {
        archiveProgress.value = progress.message;
      }
    });
    const outcome = await api.archiveIteration({
      iteration,
      confirmation: payload.confirmation,
      force: payload.force,
    });
    archiveState.value = null;
    workspaceList.value?.clearMergeResult(iteration);
    if (outcome.archived) {
      toast(`归档完成：已移除 ${outcome.removedCount} 个 worktree`);
    } else {
      toast(`归档未完成：${outcome.failed.length} 条失败，可修正后重试`, "warning");
    }
    await loadGroups(false);
  } catch (err) {
    handleError(err);
  } finally {
    archiveUnlisten?.();
    archiveUnlisten = null;
    archiveProgress.value = null;
  }
}

/** 备注保存：用返回值就地更新该迭代（不重新拉列表，设计 009 §3.3） */
async function saveNote(payload: { iteration: string; note: string | null }): Promise<void> {
  try {
    const saved = await api.setIterationNote(payload.iteration, payload.note);
    groups.value = groups.value.map((group) =>
      group.iteration === payload.iteration ? { ...group, note: saved } : group,
    );
    toast(saved === null ? "备注已清除" : "备注已保存");
  } catch (err) {
    handleError(err);
  }
}

async function setHidden(iteration: string, hidden: boolean): Promise<void> {
  try {
    // 用返回值就地更新 hiddenAt，卡片在主列表与收纳区之间移动（workflows.md §9 第 5、7 步）
    const hiddenAt = await api.setIterationHidden(iteration, hidden);
    groups.value = groups.value.map((group) =>
      group.iteration === iteration ? { ...group, hiddenAt } : group,
    );
    toast(hidden ? `已隐藏 ${iteration}` : `已恢复 ${iteration}`);
  } catch (err) {
    handleError(err);
  }
}

// ---------------- 移除 ----------------

async function requestRemoval(payload: { iteration: string; project: WorkspaceProject }): Promise<void> {
  const { iteration, project } = payload;
  try {
    const assessment =
      project.validity === "discovered"
        ? await api.assessDiscoveredRemoval(iteration, project.worktreePath, project.sourceRepository)
        : await api.assessRemoval(iteration, project.projectId, project.worktreePath);
    removalState.value = { iteration, project, assessment };
  } catch (err) {
    handleError(err);
  }
}

async function confirmRemoval(payload: {
  removeCopiedVendor: boolean;
  confirmation: string;
}): Promise<void> {
  const state = removalState.value;
  if (!state) {
    return;
  }
  const { iteration, project } = state;
  try {
    if (project.validity === "discovered") {
      await api.removeDiscoveredWorktree({
        iteration,
        projectId: project.projectId,
        worktreePath: project.worktreePath,
        sourcePath: project.sourceRepository,
        confirmation: payload.confirmation,
        removeCopiedVendor: payload.removeCopiedVendor,
      });
    } else {
      await api.removeWorktree({
        iteration,
        projectId: project.projectId,
        worktreePath: project.worktreePath,
        confirmation: payload.confirmation,
        removeCopiedVendor: payload.removeCopiedVendor,
      });
    }
    removalState.value = null;
    workspaceList.value?.clearMergeResult(iteration);
    toast("worktree 已移除");
    await loadGroups(false);
  } catch (err) {
    handleError(err);
    // 风险评估过期时重新评估，让用户看到最新风险
    try {
      await requestRemoval({ iteration, project });
    } catch {
      // 忽略二次评估失败
    }
  }
}
</script>

<template>
  <div class="app-shell">
    <AppSidebar :page="page" @navigate="navigate" />
    <main class="app-main">
      <div v-if="banner" class="app-banner" :class="banner.tone">
        <span>{{ banner.message }}</span>
        <button type="button" class="icon-btn" aria-label="关闭提示" @click="banner = null">×</button>
      </div>
      <div class="content">
        <WorkspaceList
          v-if="page === 'workspaces'"
          ref="workspaceList"
          :groups="groups"
          :reconcile-loading="reconcileLoading"
          :empty-message="emptyMessage"
          @create="page = 'create'"
          @reconcile="reconcile"
          @open-iteration="openIteration"
          @open-project="openProject"
          @remove="requestRemoval"
          @archive="requestArchive"
          :archive-assessing="archiveAssessing"
          @save-note="saveNote"
          @hide="(iteration) => setHidden(iteration, true)"
          @restore="(iteration) => setHidden(iteration, false)"
          @changed="loadGroups(false)"
        />
        <OperationView
          v-else-if="page === 'create' && operationState"
          :items="operationState.items"
          :running="operationState.running"
          :result="operationState.result"
          :error-message="operationState.errorMessage"
          @back="backToWorkspaces"
        />
        <CreateWorkspace
          v-else-if="page === 'create'"
          :config="config ?? EMPTY_CONFIG"
          @start="startCreate"
          @back="page = 'workspaces'"
        />
        <SettingsView v-else :config="config ?? EMPTY_CONFIG" @save="saveConfigValue" />
      </div>
    </main>

    <RemovalDialog
      v-if="removalState"
      :iteration="removalState.iteration"
      :project="removalState.project"
      :assessment="removalState.assessment"
      @confirm="confirmRemoval"
      @cancel="removalState = null"
    />
    <MergeDetailDialog
      v-if="mergeDetailState"
      :iteration="mergeDetailState.iteration"
      :project="mergeDetailState.project"
      :target="mergeDetailState.target"
      :cell="mergeDetailState.cell"
      @close="mergeDetailState = null"
    />
    <ArchiveDialog
      v-if="archiveState"
      :iteration="archiveState.iteration"
      :assessment="archiveState.assessment"
      :progress-line="archiveProgress"
      @confirm="confirmArchive"
      @cancel="archiveState = null"
    />

    <ToastStack :toasts="toasts" @dismiss="dismissToast" />
  </div>
</template>

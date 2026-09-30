<script setup lang="ts">
// 应用壳（architecture.md §8）：页面状态机（workspaces | create | settings）、侧栏、顶部横幅、toast、模态层挂载点。
// S1 只做静态还原 + 读命令（配置 / 快扫）；各操作命令在 S3 起按步骤接线。
import { computed, onMounted, provide, ref } from "vue";
import * as api from "./api/tauri";
import AppSidebar from "./components/AppSidebar.vue";
import ToastStack from "./components/ToastStack.vue";
import WorkspaceList from "./components/WorkspaceList.vue";
import CreateWorkspace from "./components/CreateWorkspace.vue";
import SettingsView from "./components/SettingsView.vue";
import type { AppConfig, MergeCheckResult, PageKey, ToastTone, WorkspaceGroup } from "./types";
import { UI_CONTEXT_KEY } from "./utils/context";
import type { BannerTone } from "./utils/context";

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
/** S1 阶段合并检查结果为空；S5 起由 WorkspaceList 持有真实结果（architecture.md §8） */
const mergeResults = new Map<string, MergeCheckResult>();
let toastSeq = 0;

const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
if (!isTauriEnv) {
  banner.value = { tone: "demo", message: "浏览器演示模式：数据为只读假数据" };
}

const emptyMessage = computed(() =>
  config.value?.workspaceRoot
    ? "还没有任何迭代；点击「新建工作区」开始。"
    : "请先在设置中填写工作区根目录",
);

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

function navigate(next: PageKey): void {
  page.value = next;
}

async function loadConfig(): Promise<void> {
  try {
    config.value = await api.getConfig();
  } catch (err) {
    const error = api.toAppError(err);
    setBanner(error.message, "error");
  }
}

async function loadGroups(reconcile: boolean): Promise<void> {
  try {
    groups.value = await api.listWorkspaces(reconcile);
  } catch (err) {
    const error = api.toAppError(err);
    setBanner(error.message, "error");
  }
}

onMounted(async () => {
  await loadConfig();
  await loadGroups(false);
});
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
          :groups="groups"
          :reconcile-loading="reconcileLoading"
          :empty-message="emptyMessage"
          :merge-results="mergeResults"
        />
        <CreateWorkspace v-else-if="page === 'create'" :config="config ?? EMPTY_CONFIG" />
        <SettingsView v-else :config="config ?? EMPTY_CONFIG" />
      </div>
    </main>
    <ToastStack :toasts="toasts" @dismiss="dismissToast" />
  </div>
</template>

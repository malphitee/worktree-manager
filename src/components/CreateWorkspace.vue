<script setup lang="ts">
// 创建页（ui-spec.md §7.2 / 设计 013）：迭代号、最近迭代胶囊、备注、统一分支名/基分支（含候选 datalist）、
// 项目勾选表（逐项目基分支 + ⟳ 刷新）、底部固定操作条。
// 联动规则（统一值覆盖已勾选行、新勾选行继承统一值、单行修改不反向影响）在 utils/create-form.ts 中实现并有单测。
import { computed, inject, onMounted, ref, watch } from "vue";
import * as api from "../api/tauri";
import type { AppConfig, CreateRequest, RemoteBranches } from "../types";
import { previewNormalized } from "../utils/base-ref";
import { precheckIteration, precheckWorkspaceRoot } from "../utils/config";
import { UI_CONTEXT_KEY } from "../utils/context";
import type { CreateProjectRow } from "../utils/create-form";
import {
  applyUnifiedBaseRef,
  applyUnifiedBranch,
  buildRows,
  toProjectRequests,
  toggleRowSelection,
} from "../utils/create-form";
import { validateNote } from "../utils/note";

const props = defineProps<{ config: AppConfig }>();

const emit = defineEmits<{
  start: [request: CreateRequest];
  back: [];
}>();

const ui = inject(UI_CONTEXT_KEY, null);

const iteration = ref("");
const note = ref("");
const unifiedBranch = ref("");
const unifiedBaseRef = ref("");
const rows = ref<CreateProjectRow[]>(buildRows(props.config.projects));
/** projectId → 基分支候选（本地 / 远端），离开创建页即丢弃（设计 013） */
const candidates = ref<Map<string, RemoteBranches>>(new Map());
const refreshing = ref<string[]>([]);

watch(
  () => props.config.projects,
  (projects) => {
    rows.value = buildRows(projects);
  },
);

onMounted(async () => {
  // 进入页面：对所有已配置项目加载本地候选（只读 refs/remotes，不联网）
  await Promise.all(props.config.projects.map((project) => loadCandidates(project.id, false)));
});

const selectedRows = computed(() => rows.value.filter((row) => row.selected));

const iterationError = computed(() => precheckIteration(iteration.value));
const rootError = computed(() => precheckWorkspaceRoot(props.config.workspaceRoot ?? ""));
const noteError = computed(() => {
  if (note.value.trim().length === 0) {
    return null;
  }
  const result = validateNote(note.value);
  return result.ok ? null : result.message;
});

const disabledReason = computed<string | null>(() => {
  if (rootError.value) {
    return rootError.value;
  }
  if (iterationError.value) {
    return iterationError.value;
  }
  if (noteError.value) {
    return noteError.value;
  }
  if (selectedRows.value.length === 0) {
    return "请至少勾选一个项目";
  }
  return null;
});

/** 统一基分支候选：所有已勾选项目候选的并集 */
const unifiedCandidates = computed<string[]>(() => {
  const set = new Set<string>();
  for (const row of rows.value) {
    if (!row.selected) {
      continue;
    }
    const candidate = candidates.value.get(row.projectId);
    candidate?.branches.forEach((branch) => set.add(branch));
  }
  return [...set].sort();
});

function remotesOf(projectId: string): string[] {
  return candidates.value.get(projectId)?.remotes ?? [];
}

function warningOf(projectId: string): string | null {
  return candidates.value.get(projectId)?.warning ?? null;
}

function previewOf(input: string): string {
  const remotes = [...new Set(selectedRows.value.flatMap((row) => remotesOf(row.projectId)))];
  const preview = previewNormalized(input, remotes);
  return preview.error === null ? `将解析为：${preview.value}` : preview.error;
}

function onUnifiedBranchInput(): void {
  applyUnifiedBranch(rows.value, unifiedBranch.value);
}

function onUnifiedBaseRefInput(): void {
  applyUnifiedBaseRef(rows.value, unifiedBaseRef.value);
}

function onToggleRow(row: CreateProjectRow): void {
  toggleRowSelection(row, unifiedBranch.value, unifiedBaseRef.value);
}

function applyRecent(value: string): void {
  iteration.value = value;
}

/** 单项目刷新：`ls-remote --heads`（15 秒超时；失败降级为本地候选并显示 warning） */
async function loadCandidates(projectId: string, includeRemote: boolean): Promise<void> {
  if (refreshing.value.includes(projectId)) {
    return;
  }
  refreshing.value = [...refreshing.value, projectId];
  try {
    const result = await api.listRemoteBranches(projectId, includeRemote);
    const next = new Map(candidates.value);
    next.set(projectId, result);
    candidates.value = next;
  } catch (err) {
    ui?.toast(api.toAppError(err).message, "danger");
  } finally {
    refreshing.value = refreshing.value.filter((item) => item !== projectId);
  }
}

/** 整页「⟳ 全部」：串行刷新已勾选项目 */
async function refreshAll(): Promise<void> {
  for (const row of rows.value) {
    if (row.selected) {
      await loadCandidates(row.projectId, true);
    }
  }
}

function start(): void {
  if (disabledReason.value) {
    return;
  }
  const noteResult = validateNote(note.value);
  const request: CreateRequest = {
    iteration: iteration.value,
    unifiedBranch: unifiedBranch.value.trim().length > 0 ? unifiedBranch.value.trim() : null,
    unifiedBaseRef: unifiedBaseRef.value,
    projects: toProjectRequests(rows.value),
  };
  // 备注三态：空输入＝缺省（不改动）；有值＝设置（后端会再次校验）
  if (noteResult.value !== null) {
    request.note = noteResult.value;
  }
  emit("start", request);
}
</script>

<template>
  <div>
    <div class="page-head">
      <h1>新建工作区</h1>
    </div>

    <div class="create-grid">
      <div class="card">
        <h2>迭代</h2>
        <label class="field">
          迭代号<span class="required-mark">*</span>
          <input v-model="iteration" type="text" placeholder="如 7.3.0 / sprint-42" />
        </label>
        <div v-if="props.config.recentIterations.length > 0" class="chip-row">
          <button
            v-for="recent in props.config.recentIterations"
            :key="recent"
            type="button"
            class="chip"
            @click="applyRecent(recent)"
          >{{ recent }}</button>
        </div>
        <label class="field" style="margin-top: 12px">
          备注（可选，≤50 字符）
          <input v-model="note" type="text" :maxlength="50" placeholder="如：等 QA 回归后归档" />
        </label>
        <p v-if="iterationError" class="field-error">{{ iterationError }}</p>
      </div>

      <div class="card">
        <h2>统一分支 / 基分支</h2>
        <label class="field">
          统一分支名（留空 ＝ Detached HEAD）
          <input v-model="unifiedBranch" type="text" placeholder="如 feature/7.3.0" @input="onUnifiedBranchInput" />
        </label>
        <label class="field" style="margin-top: 12px">
          统一基分支（留空 ＝ origin/master）
          <span class="field-row">
            <input
              v-model="unifiedBaseRef"
              type="text"
              list="unified-base-ref"
              placeholder="origin/master"
              @input="onUnifiedBaseRefInput"
            />
            <button
              type="button"
              class="icon-btn"
              title="串行刷新所有已勾选项目的远端分支候选"
              :disabled="props.config.projects.length === 0"
              @click="refreshAll"
            >⟳ 全部</button>
          </span>
          <datalist id="unified-base-ref">
            <option v-for="branch in unifiedCandidates" :key="branch" :value="branch" />
          </datalist>
        </label>
        <p class="hint">{{ previewOf(unifiedBaseRef) }}</p>
      </div>
    </div>

    <div class="card">
      <div class="page-head">
        <h2 style="margin: 0">项目</h2>
        <span class="spacer" />
        <span class="hint">勾选项目后可用「⟳」逐个拉取远端候选</span>
      </div>
      <table class="select-table">
        <thead>
          <tr>
            <th style="width: 44px">选择</th>
            <th>项目</th>
            <th style="width: 80px">类型</th>
            <th style="width: 96px">vendor</th>
            <th>分支名</th>
            <th>基分支</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.projectId">
            <td>
              <input type="checkbox" :checked="row.selected" :aria-label="`选择 ${row.projectId}`" @change="onToggleRow(row)" />
            </td>
            <td>{{ row.projectId }}</td>
            <td>{{ row.projectType }}</td>
            <td>{{ row.vendorAvailable ? "有" : "无" }}</td>
            <td>
              <input v-model="row.branch" type="text" placeholder="留空 ＝ Detached" />
            </td>
            <td>
              <span class="field-row">
                <input
                  v-model="row.baseRef"
                  type="text"
                  :list="`base-ref-${row.projectId}`"
                  placeholder="origin/master"
                />
                <button
                  type="button"
                  class="icon-btn"
                  title="拉取该项目的远端分支候选"
                  :disabled="refreshing.includes(row.projectId)"
                  @click="loadCandidates(row.projectId, true)"
                >⟳</button>
                <span
                  v-if="warningOf(row.projectId)"
                  class="hint"
                  :title="warningOf(row.projectId) ?? ''"
                >⚠</span>
              </span>
              <datalist :id="`base-ref-${row.projectId}`">
                <option
                  v-for="branch in candidates.get(row.projectId)?.branches ?? []"
                  :key="branch"
                  :value="branch"
                />
              </datalist>
              <span class="cell-sub">{{ previewOf(row.baseRef) }}</span>
            </td>
          </tr>
          <tr v-if="rows.length === 0">
            <td colspan="6" class="empty-state">还没有配置项目，请先到设置页添加。</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>

  <div class="create-footer">
    <span v-if="disabledReason" class="hint">{{ disabledReason }}</span>
    <button type="button" class="btn" @click="emit('back')">返回</button>
    <button type="button" class="btn primary" :disabled="disabledReason !== null" @click="start">开始创建</button>
  </div>
</template>


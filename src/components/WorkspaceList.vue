<script setup lang="ts">
// 工作区列表页（ui-spec.md §7.1）：搜索、列显隐、迭代卡片、已隐藏迭代收纳区、「复核状态」按钮。
// 合并检查结果只存内存（architecture.md §8），订阅 merge-check-progress 后逐行就地更新（003）。
import { computed, inject, onBeforeUnmount, onMounted, ref } from "vue";
import * as api from "../api/tauri";
import type { MergeCheckProgress, MergeCheckResult, WorkspaceGroup, WorkspaceProject } from "../types";
import { COLUMNS, LOCKED_COLUMN_KEYS, resolveVisibleColumns } from "../utils/columns";
import { UI_CONTEXT_KEY } from "../utils/context";
import type { MergeTarget } from "../utils/merge";
import { applyRecord } from "../utils/merge";
import {
  readExpandedIterations,
  readHiddenZoneExpanded,
  readVisibleColumns,
  writeExpandedIterations,
  writeHiddenZoneExpanded,
  writeVisibleColumns,
} from "../utils/storage";
import IterationCard from "./IterationCard.vue";

const props = defineProps<{
  groups: WorkspaceGroup[];
  reconcileLoading: boolean;
  emptyMessage: string;
  /** 正在评估归档的迭代号 */
  archiveAssessing: string | null;
}>();

const emit = defineEmits<{
  create: [];
  reconcile: [];
  openIteration: [iteration: string];
  archive: [iteration: string];
  hide: [iteration: string];
  restore: [iteration: string];
  saveNote: [payload: { iteration: string; note: string | null }];
  openProject: [payload: { iteration: string; project: WorkspaceProject }];
  remove: [payload: { iteration: string; project: WorkspaceProject }];
  mergeDetail: [payload: { iteration: string; project: WorkspaceProject; target: MergeTarget }];
  /** 排序 / 移动成功后请求父组件重新快扫 */
  changed: [];
}>();

const ui = inject(UI_CONTEXT_KEY, null);

const search = ref("");
const columnsPanelOpen = ref(false);
const columnsWrap = ref<HTMLElement | null>(null);
const visibleColumns = ref<string[]>(resolveVisibleColumns(readVisibleColumns()));
const expandedIterations = ref<Set<string>>(new Set(readExpandedIterations() ?? []));
const expandedAll = ref(readExpandedIterations() === null);
const hiddenZoneExpanded = ref(readHiddenZoneExpanded());

/** 合并检查结果（仅内存，随组件生命周期；不落 localStorage、不落清单） */
const mergeResults = ref<Map<string, MergeCheckResult>>(new Map());
const checking = ref<string[]>([]);
const progressLines = ref<Map<string, string>>(new Map());
let unlisten: (() => void) | null = null;

const filterActive = computed(() => search.value.trim().length > 0);

const mainGroups = computed(() =>
  props.groups.filter((group) => group.hiddenAt === null && matchesFilter(group)),
);

const hiddenGroups = computed(() => props.groups.filter((group) => group.hiddenAt !== null));

function matchesFilter(group: WorkspaceGroup): boolean {
  const keyword = search.value.trim().toLowerCase();
  if (keyword.length === 0) {
    return true;
  }
  if (group.iteration.toLowerCase().includes(keyword)) {
    return true;
  }
  return group.projects.some(
    (project) =>
      project.projectId.toLowerCase().includes(keyword) ||
      project.branchDisplay.toLowerCase().includes(keyword),
  );
}

function isExpanded(group: WorkspaceGroup): boolean {
  return expandedAll.value || expandedIterations.value.has(group.iteration);
}

function toggleExpanded(iteration: string): void {
  const next = new Set(expandedAll.value ? props.groups.map((group) => group.iteration) : expandedIterations.value);
  if (next.has(iteration)) {
    next.delete(iteration);
  } else {
    next.add(iteration);
  }
  expandedAll.value = false;
  expandedIterations.value = next;
  writeExpandedIterations([...next]);
}

function toggleColumn(key: string): void {
  if (LOCKED_COLUMN_KEYS.includes(key)) {
    return;
  }
  const next = visibleColumns.value.includes(key)
    ? visibleColumns.value.filter((item) => item !== key)
    : COLUMNS.map((column) => column.key).filter(
        (item) => visibleColumns.value.includes(item) || item === key,
      );
  visibleColumns.value = resolveVisibleColumns(next);
  writeVisibleColumns(visibleColumns.value);
}

function toggleHiddenZone(): void {
  hiddenZoneExpanded.value = !hiddenZoneExpanded.value;
  writeHiddenZoneExpanded(hiddenZoneExpanded.value);
}

function onDocumentClick(event: MouseEvent): void {
  if (!columnsPanelOpen.value) {
    return;
  }
  const target = event.target;
  if (columnsWrap.value && target instanceof Node && columnsWrap.value.contains(target)) {
    return;
  }
  columnsPanelOpen.value = false;
}

// ------------------------------ 拖拽排序 / 跨迭代移动（014） ------------------------------

const dragSource = ref<{ iteration: string; worktreePath: string } | null>(null);
const dropTarget = ref<{ iteration: string; beforeWorktreePath: string | null } | null>(null);

function onDragStart(payload: { iteration: string; worktreePath: string }): void {
  dragSource.value = payload;
}

function onDragEnd(): void {
  dragSource.value = null;
  dropTarget.value = null;
}

function onDragOver(payload: { iteration: string; beforeWorktreePath: string | null }): void {
  if (!dragSource.value) {
    return;
  }
  dropTarget.value = payload;
}

/** 落点是否等于原位（锚点是自身或自身的下一行） */
function isSamePosition(beforeWorktreePath: string | null): boolean {
  const source = dragSource.value;
  if (!source) {
    return true;
  }
  const group = props.groups.find((item) => item.iteration === source.iteration);
  const paths = (group?.projects ?? [])
    .filter((project) => project.lifecycle === "active")
    .map((project) => project.worktreePath);
  const index = paths.indexOf(source.worktreePath);
  if (index < 0) {
    return false;
  }
  if (beforeWorktreePath === source.worktreePath) {
    return true;
  }
  return (paths[index + 1] ?? null) === beforeWorktreePath;
}

async function onDrop(payload: { iteration: string; beforeWorktreePath: string | null }): Promise<void> {
  const source = dragSource.value;
  dragSource.value = null;
  dropTarget.value = null;
  if (!source) {
    return;
  }
  // 落点是原位 → 不发请求（ui-spec.md §7.5）
  if (source.iteration === payload.iteration && isSamePosition(payload.beforeWorktreePath)) {
    return;
  }
  try {
    if (source.iteration === payload.iteration) {
      await api.reorderProject(source.iteration, source.worktreePath, payload.beforeWorktreePath);
      // 同迭代排序不改变记录结论，合并检查结果保留（设计 004 决策 10）
    } else {
      await api.moveProject({
        iteration: source.iteration,
        worktreePath: source.worktreePath,
        targetIteration: payload.iteration,
        beforeWorktreePath: payload.beforeWorktreePath,
      });
      // 跨迭代移动后清除源与目标的合并结论（设计 001 §3.4）
      clearMergeResult(source.iteration);
      clearMergeResult(payload.iteration);
    }
    emit("changed");
  } catch (err) {
    ui?.toast(api.toAppError(err).message, "danger");
  }
}

// ------------------------------ 合并检查（001/003） ------------------------------

function applyProgress(progress: MergeCheckProgress): void {
  const lines = new Map(progressLines.value);
  if (progress.message) {
    lines.set(progress.iteration, progress.message);
  }
  progressLines.value = lines;
  mergeResults.value = applyRecord(mergeResults.value, progress);
}

async function runMergeCheck(iteration: string): Promise<void> {
  if (checking.value.includes(iteration)) {
    return;
  }
  checking.value = [...checking.value, iteration];
  try {
    // 返回值是完整结果，覆盖事件累积的中间态（事件可能丢失或乱序）
    const result = await api.checkMergeStatus(iteration);
    const next = new Map(mergeResults.value);
    next.set(iteration, result);
    mergeResults.value = next;
  } catch (err) {
    ui?.toast(api.toAppError(err).message, "danger");
  } finally {
    checking.value = checking.value.filter((item) => item !== iteration);
    const lines = new Map(progressLines.value);
    lines.delete(iteration);
    progressLines.value = lines;
  }
}

/** 供 App 在移除 / 归档 / 跨迭代移动 / 创建后清除该迭代的内存结论（001 §3.4） */
function clearMergeResult(iteration: string): void {
  const next = new Map(mergeResults.value);
  next.delete(iteration);
  mergeResults.value = next;
}

defineExpose({ clearMergeResult });

onMounted(async () => {
  document.addEventListener("click", onDocumentClick);
  // 订阅一次，组件卸载时释放（003 决策 8）
  unlisten = await api.onMergeCheckProgress(applyProgress);
});

onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
});
</script>

<template>
  <div>
    <div class="page-head">
      <h1>工作区</h1>
      <input
        v-model="search"
        class="search-input"
        type="text"
        placeholder="搜索迭代号 / 项目 / 分支"
        aria-label="搜索"
      />
      <div ref="columnsWrap" class="columns-panel-wrap">
        <button
          type="button"
          class="btn"
          :class="{ disabled: false }"
          @click.stop="columnsPanelOpen = !columnsPanelOpen"
        >列</button>
        <div v-if="columnsPanelOpen" class="columns-panel" @click.stop>
          <label v-for="column in COLUMNS" :key="column.key" :class="{ locked: column.locked }">
            <input
              type="checkbox"
              :checked="column.locked || visibleColumns.includes(column.key)"
              :disabled="column.locked"
              @change="toggleColumn(column.key)"
            />
            <span>{{ column.label }}</span>
          </label>
        </div>
      </div>
      <span class="spacer" />
      <button type="button" class="btn" :disabled="props.reconcileLoading" @click="emit('reconcile')">
        {{ props.reconcileLoading ? "复核中…" : "复核状态" }}
      </button>
      <button type="button" class="btn primary" @click="emit('create')">新建工作区</button>
    </div>

    <div v-if="props.groups.length === 0" class="empty-state">{{ props.emptyMessage }}</div>

    <div v-else-if="mainGroups.length === 0 && hiddenGroups.length === 0" class="empty-state">
      没有符合搜索条件的迭代
    </div>

    <IterationCard
      v-for="group in mainGroups"
      :key="group.iteration"
      :group="group"
      :visible-columns="visibleColumns"
      :expanded="isExpanded(group)"
      :filter-active="filterActive"
      :merge-result="mergeResults.get(group.iteration) ?? null"
      :checking="checking.includes(group.iteration)"
      :progress-line="progressLines.get(group.iteration) ?? null"
      :dragging="false"
      :archiving="props.archiveAssessing === group.iteration"
      :drag-source-path="dragSource?.iteration === group.iteration ? dragSource.worktreePath : null"
      :drop-armed="dropTarget?.iteration === group.iteration"
      :drop-before-path="dropTarget?.iteration === group.iteration ? dropTarget.beforeWorktreePath : null"
      @toggle="toggleExpanded"
      @open-iteration="(iteration) => emit('openIteration', iteration)"
      @archive="(iteration) => emit('archive', iteration)"
      @hide="(iteration) => emit('hide', iteration)"
      @check-merge="runMergeCheck"
      @drag-start="onDragStart"
      @drag-end="onDragEnd"
      @drag-over="onDragOver"
      @drop="onDrop"
      @save-note="(payload) => emit('saveNote', payload)"
      @open-project="(payload) => emit('openProject', payload)"
      @remove="(payload) => emit('remove', payload)"
      @merge-detail="(payload) => emit('mergeDetail', payload)"
    />

    <div v-if="hiddenGroups.length > 0" class="hidden-zone">
      <button type="button" class="hidden-zone-head" @click="toggleHiddenZone">
        <span>{{ hiddenZoneExpanded ? "▾" : "▸" }}</span>
        <span>已隐藏迭代（{{ hiddenGroups.length }}）</span>
      </button>
      <div v-if="hiddenZoneExpanded">
        <div v-for="group in hiddenGroups" :key="group.iteration" class="hidden-item">
          <span class="iteration-title">{{ group.iteration }}</span>
          <span class="iteration-path mono" :title="group.iterationPath">{{ group.iterationPath }}</span>
          <button type="button" class="btn" @click="emit('restore', group.iteration)">恢复</button>
        </div>
      </div>
    </div>
  </div>
</template>


<script setup lang="ts">
// 工作区列表页（ui-spec.md §7.1）：搜索、列显隐、迭代卡片、已隐藏迭代收纳区、「复核状态」按钮。
// 合并检查结果只存内存（architecture.md §8），由 App.vue 传入。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { MergeCheckResult, WorkspaceGroup, WorkspaceProject } from "../types";
import { COLUMNS, LOCKED_COLUMN_KEYS, resolveVisibleColumns } from "../utils/columns";
import type { MergeTarget } from "../utils/merge";
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
  mergeResults: Map<string, MergeCheckResult>;
}>();

const emit = defineEmits<{
  create: [];
  reconcile: [];
  openIteration: [iteration: string];
  archive: [iteration: string];
  hide: [iteration: string];
  restore: [iteration: string];
  checkMerge: [iteration: string];
  saveNote: [payload: { iteration: string; note: string | null }];
  openProject: [payload: { iteration: string; project: WorkspaceProject }];
  remove: [payload: { iteration: string; project: WorkspaceProject }];
  mergeDetail: [payload: { iteration: string; project: WorkspaceProject; target: MergeTarget }];
  copyCell: [text: string];
}>();

const search = ref("");
const columnsPanelOpen = ref(false);
const columnsWrap = ref<HTMLElement | null>(null);
const visibleColumns = ref<string[]>(resolveVisibleColumns(readVisibleColumns()));
const expandedIterations = ref<Set<string>>(new Set(readExpandedIterations() ?? []));
const expandedAll = ref(readExpandedIterations() === null);
const hiddenZoneExpanded = ref(readHiddenZoneExpanded());

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

onMounted(() => {
  document.addEventListener("click", onDocumentClick);
});

onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
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
      :merge-result="props.mergeResults.get(group.iteration) ?? null"
      @toggle="toggleExpanded"
      @open-iteration="(iteration) => emit('openIteration', iteration)"
      @archive="(iteration) => emit('archive', iteration)"
      @hide="(iteration) => emit('hide', iteration)"
      @check-merge="(iteration) => emit('checkMerge', iteration)"
      @save-note="(payload) => emit('saveNote', payload)"
      @open-project="(payload) => emit('openProject', payload)"
      @remove="(payload) => emit('remove', payload)"
      @merge-detail="(payload) => emit('mergeDetail', payload)"
      @copy-cell="(text) => emit('copyCell', text)"
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


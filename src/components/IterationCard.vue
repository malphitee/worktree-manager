<script setup lang="ts">
// 单个迭代卡片（ui-spec.md §4 / §7.1）：头部按钮组、清单健康度横条、公共目录状态、项目表格。
// 组件只负责展示与 emit，不自行推导业务结论（architecture.md §4.3）。
import { computed, ref } from "vue";
import type { MergeCheckResult, MergeRecordResult, WorkspaceGroup, WorkspaceProject } from "../types";
import { COLUMNS } from "../utils/columns";
import type { MergeTarget } from "../utils/merge";
import { MERGE_NOT_CHECKED_LABEL, mergeCellLabel, mergeCellTone } from "../utils/merge";
import { validateNote } from "../utils/note";
import {
  CHANGES_LABELS,
  RENAMED_LABEL,
  STALE_LABEL,
  hasChangesLabel,
  hasChangesTone,
  sharedDirLabel,
  sharedDirTone,
  validityLabel,
  validityTone,
  vendorLabel,
  vendorTone,
} from "../utils/status";
import NoteTooltip from "./NoteTooltip.vue";

const props = defineProps<{
  group: WorkspaceGroup;
  visibleColumns: string[];
  expanded: boolean;
  /** 搜索过滤中：手柄置灰不可拖 */
  filterActive: boolean;
  /** 该迭代的合并检查结果（内存态，006 用于就地覆盖行） */
  mergeResult: MergeCheckResult | null;
}>();

const emit = defineEmits<{
  toggle: [iteration: string];
  openIteration: [iteration: string];
  archive: [iteration: string];
  hide: [iteration: string];
  checkMerge: [iteration: string];
  saveNote: [payload: { iteration: string; note: string | null }];
  openProject: [payload: { iteration: string; project: WorkspaceProject }];
  remove: [payload: { iteration: string; project: WorkspaceProject }];
  mergeDetail: [payload: { iteration: string; project: WorkspaceProject; target: MergeTarget }];
  copyCell: [text: string];
}>();

const editingNote = ref(false);
const noteDraft = ref("");
const noteError = ref<string | null>(null);
const noteMarker = ref<HTMLElement | null>(null);
const noteHovering = ref(false);

const manifestValid = computed(() => props.group.manifestHealth === "valid");
const canEditNote = computed(() => manifestValid.value);

function show(key: string): boolean {
  return props.visibleColumns.includes(key);
}

function toggleExpanded(): void {
  emit("toggle", props.group.iteration);
}

function startNoteEdit(): void {
  if (!canEditNote.value) {
    return;
  }
  noteDraft.value = props.group.note ?? "";
  noteError.value = null;
  editingNote.value = true;
}

function cancelNoteEdit(): void {
  editingNote.value = false;
  noteError.value = null;
}

function saveNote(): void {
  const result = validateNote(noteDraft.value);
  if (!result.ok) {
    noteError.value = result.message;
    return;
  }
  editingNote.value = false;
  emit("saveNote", { iteration: props.group.iteration, note: result.value });
}

function shortCommit(commit: string | null): string {
  return commit ? commit.slice(0, 7) : "";
}

function formatCreatedAt(iso: string | null): string {
  if (!iso) {
    return "—";
  }
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** 该行是否有合并检查结论（006：优先使用合并检查的 hasChanges / dirty） */
function mergeRecordOf(project: WorkspaceProject): MergeRecordResult | null {
  if (!props.mergeResult) {
    return null;
  }
  return (
    props.mergeResult.records.find(
      (record) => record.projectId === project.projectId && record.worktreePath === project.worktreePath,
    ) ?? null
  );
}

function rowHasChanges(project: WorkspaceProject): boolean | null {
  const record = mergeRecordOf(project);
  return record ? record.hasChanges : project.hasChanges;
}

function rowDirty(project: WorkspaceProject): boolean | null {
  const record = mergeRecordOf(project);
  return record ? record.dirty : project.dirty;
}

function cellOf(project: WorkspaceProject, target: MergeTarget): MergeRecordResult["develop"] | null {
  const record = mergeRecordOf(project);
  return record ? record[target] : null;
}

interface MergeCellView {
  label: string;
  tone: string;
  stale: boolean;
}

/** 合并单元格视图（无结论 → null，前端不推导） */
function mergeCellView(project: WorkspaceProject, target: MergeTarget): MergeCellView | null {
  const cell = cellOf(project, target);
  if (!cell) {
    return null;
  }
  return { label: mergeCellLabel(cell.status), tone: mergeCellTone(cell.status), stale: cell.stale };
}
</script>

<template>
  <section class="iteration-card">
    <header class="iteration-head">
      <button type="button" class="icon-btn collapse-btn" aria-label="折叠" @click="toggleExpanded">
        {{ props.expanded ? "▾" : "▸" }}
      </button>
      <span class="iteration-title">{{ props.group.iteration }}</span>
      <span
        v-if="props.group.note"
        ref="noteMarker"
        class="note-marker"
        aria-label="备注"
        @mouseenter="noteHovering = true"
        @mouseleave="noteHovering = false"
      >▤</span>
      <span class="iteration-path mono" :title="props.group.iterationPath">{{ props.group.iterationPath }}</span>
      <span class="spacer" />
      <div class="head-actions">
        <button type="button" class="btn" :disabled="!canEditNote" @click="startNoteEdit">✎ 备注</button>
        <button type="button" class="btn" :disabled="!manifestValid" @click="emit('hide', props.group.iteration)">隐藏</button>
        <button type="button" class="btn" :disabled="!manifestValid" @click="emit('archive', props.group.iteration)">归档</button>
        <button type="button" class="btn" :disabled="!props.group.openable" @click="emit('openIteration', props.group.iteration)">
          打开目录
        </button>
        <button type="button" class="btn" :disabled="!manifestValid" @click="emit('checkMerge', props.group.iteration)">
          检查合并
        </button>
      </div>
    </header>

    <NoteTooltip v-if="noteHovering && props.group.note" :text="props.group.note" :anchor="noteMarker" />

    <div v-if="props.group.manifestHealth !== 'valid'" class="manifest-bar">
      <span>清单不可用（{{ props.group.manifestHealth }}）：{{ props.group.manifestMessage }}</span>
    </div>

    <div v-if="editingNote" class="note-editor">
      <input
        v-model="noteDraft"
        type="text"
        :maxlength="50"
        placeholder="备注（≤50 字符，单行）"
        @keydown.enter.prevent="saveNote"
        @keydown.esc.prevent="cancelNoteEdit"
      />
      <button type="button" class="btn primary" @click="saveNote">保存</button>
      <button type="button" class="btn" @click="cancelNoteEdit">取消</button>
      <span v-if="noteError" class="field-error">{{ noteError }}</span>
    </div>

    <div v-if="props.expanded">
      <div v-if="props.group.sharedDirectories.length > 0" class="shared-dir-row">
        <span>公共目录：</span>
        <span
          v-for="rule in props.group.sharedDirectories"
          :key="rule.targetDirectory"
          class="tag"
          :class="sharedDirTone(rule.status)"
          :title="rule.message ?? rule.targetPath"
        >{{ rule.targetDirectory }} · {{ sharedDirLabel(rule.status) }}</span>
      </div>

      <div class="table-scroll">
        <table class="project-table">
          <thead>
            <tr>
              <th v-for="column in COLUMNS" v-show="column.locked || show(column.key)" :key="column.key">
                {{ column.label }}
              </th>
            </tr>
          </thead>
          <tbody>

            <tr v-for="project in props.group.projects" :key="`${project.projectId}-${project.worktreePath}`">
              <td v-show="show('project')">
                <span
                  v-if="project.lifecycle === 'active'"
                  class="drag-handle"
                  :class="{ disabled: props.filterActive }"
                  title="拖拽排序"
                >⠿</span>
                <span>{{ project.projectId }}</span>
                <span v-if="project.renamedFrom" class="renamed-flag" :title="`原名 ${project.renamedFrom}`">
                  {{ RENAMED_LABEL }}
                </span>
              </td>
              <td v-show="show('branch')">
                <span class="cell-branch" :title="project.branchDisplay">{{ project.branchDisplay }}</span>
                <span v-if="project.baseRef" class="cell-sub">基于 {{ project.baseRef }}</span>
              </td>
              <td v-show="show('dirty')">
                <span class="row-flags">
                  <span class="tag" :class="hasChangesTone(rowHasChanges(project))">{{ hasChangesLabel(rowHasChanges(project)) }}</span>
                  <span v-if="rowDirty(project) === true" class="tag danger">{{ CHANGES_LABELS.dirty }}</span>
                </span>
              </td>
              <td v-show="show('mergeDevelop')">
                <span v-if="mergeCellView(project, 'develop')" class="row-flags">
                  <span
                    class="tag"
                    :class="mergeCellView(project, 'develop')!.tone"
                    @click="emit('mergeDetail', { iteration: props.group.iteration, project, target: 'develop' })"
                  >{{ mergeCellView(project, "develop")!.label }}</span>
                  <span v-if="mergeCellView(project, 'develop')!.stale" class="hint">{{ STALE_LABEL }}</span>
                </span>
                <span v-else class="tag neutral">{{ MERGE_NOT_CHECKED_LABEL }}</span>
              </td>
              <td v-show="show('mergeMaster')">
                <span v-if="mergeCellView(project, 'master')" class="row-flags">
                  <span
                    class="tag"
                    :class="mergeCellView(project, 'master')!.tone"
                    @click="emit('mergeDetail', { iteration: props.group.iteration, project, target: 'master' })"
                  >{{ mergeCellView(project, "master")!.label }}</span>
                  <span v-if="mergeCellView(project, 'master')!.stale" class="hint">{{ STALE_LABEL }}</span>
                </span>
                <span v-else class="tag neutral">{{ MERGE_NOT_CHECKED_LABEL }}</span>
              </td>
              <td v-show="show('baseCommit')">
                <span class="mono" :title="project.baseCommit ?? ''">{{ shortCommit(project.baseCommit) || "—" }}</span>
              </td>
              <td v-show="show('source')">
                <span class="cell-ellipsis" :title="project.sourceRepository">{{ project.sourceRepository || "—" }}</span>
              </td>
              <td v-show="show('worktreePath')">
                <span class="cell-ellipsis mono cell-copy" :title="project.worktreePath">{{ project.worktreePath }}</span>
              </td>
              <td v-show="show('vendor')">
                <span v-if="project.vendorStatus" class="tag" :class="vendorTone(project.vendorStatus)">
                  {{ vendorLabel(project.vendorStatus) }}
                </span>
                <span v-else>—</span>
              </td>
              <td v-show="show('createdAt')">{{ formatCreatedAt(project.createdAt) }}</td>
              <td v-show="show('status')">
                <span class="tag" :class="validityTone(project.validity)">{{ validityLabel(project.validity) }}</span>
              </td>
              <td v-show="show('actions')">
                <span class="row-flags">
                  <button
                    type="button"
                    class="icon-btn"
                    aria-label="打开"
                    :disabled="!project.openable"
                    @click="emit('openProject', { iteration: props.group.iteration, project })"
                  >打开</button>
                  <button
                    type="button"
                    class="icon-btn danger"
                    aria-label="移除"
                    :disabled="!project.removable"
                    @click="emit('remove', { iteration: props.group.iteration, project })"
                  >移除</button>
                </span>
              </td>
            </tr>
            <tr v-if="props.group.projects.length === 0">
              <td :colspan="COLUMNS.length" class="empty-state">没有可显示的项目</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>


<script setup lang="ts">
// 单个迭代卡片（ui-spec.md §4 / §7.1）：头部按钮组、清单健康度横条、公共目录状态、项目表格。
// 组件只负责展示与 emit，不自行推导业务结论（architecture.md §4.3）。
// 002：`worktreePath` / `branch` 两格单击复制；006：合并检查结果优先覆盖行内「基准变动」。
import { computed, inject, onBeforeUnmount, ref } from "vue";
import type { MergeCheckResult, MergeRecordResult, WorkspaceGroup, WorkspaceProject } from "../types";
import { copyText } from "../utils/clipboard";
import { COLUMNS } from "../utils/columns";
import { UI_CONTEXT_KEY } from "../utils/context";
import type { MergeTarget } from "../utils/merge";
import {
  MERGE_CHECKING_LABEL,
  MERGE_NOT_CHECKED_LABEL,
  mergeCellLabel,
  mergeCellTone,
} from "../utils/merge";
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
  /** 该迭代正在检查合并（003） */
  checking: boolean;
  /** fetching 阶段的一行命令提示（003 §9） */
  progressLine: string | null;
  /** 拖拽进行中：忽略复制点击（002 决策 6） */
  dragging: boolean;
  /** 正在评估归档（008）：按钮 loading */
  archiving: boolean;
  /** 正在拖拽的 worktree 路径（014，来自 WorkspaceList） */
  dragSourcePath: string | null;
  /** 本卡片是否为当前落点（014） */
  dropArmed: boolean;
  /** 落点锚点：`null` ＝ 追加到末尾；非空 ＝ 插到该行之前 */
  dropBeforePath: string | null;
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
  dragStart: [payload: { iteration: string; worktreePath: string }];
  dragEnd: [];
  dragOver: [payload: { iteration: string; beforeWorktreePath: string | null }];
  drop: [payload: { iteration: string; beforeWorktreePath: string | null }];
}>();

const ui = inject(UI_CONTEXT_KEY, null);

const editingNote = ref(false);
const noteDraft = ref("");
const noteError = ref<string | null>(null);
const copiedKey = ref<string | null>(null);
let copyTimer: ReturnType<typeof setTimeout> | null = null;

const manifestValid = computed(() => props.group.manifestHealth === "valid");
/** 只有清单有效的迭代接受落点（ui-spec.md §7.5） */
const dropAcceptable = computed(() => manifestValid.value);
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

// ------------------------------ 拖拽排序 / 跨迭代移动（014） ------------------------------

function onHandleDragStart(event: DragEvent, project: WorkspaceProject): void {
  if (props.filterActive || event.dataTransfer === null) {
    return;
  }
  event.dataTransfer.setData("text/plain", project.worktreePath);
  event.dataTransfer.effectAllowed = "move";
  emit("dragStart", { iteration: props.group.iteration, worktreePath: project.worktreePath });
}

/** 行内落点：上半部分 → 插到该行之前；下半部分 → 插到下一行之前（末尾为 null） */
function rowAnchor(event: DragEvent, project: WorkspaceProject, index: number): string | null {
  const element = event.currentTarget as HTMLElement | null;
  if (!element) {
    return null;
  }
  const rect = element.getBoundingClientRect();
  if (event.clientY < rect.top + rect.height / 2) {
    return project.worktreePath;
  }
  const next = props.group.projects[index + 1];
  return next ? next.worktreePath : null;
}

function onRowDragOver(event: DragEvent, project: WorkspaceProject, index: number): void {
  if (!dropAcceptable.value) {
    return;
  }
  event.preventDefault();
  emit("dragOver", {
    iteration: props.group.iteration,
    beforeWorktreePath: rowAnchor(event, project, index),
  });
}

function onRowDrop(event: DragEvent, project: WorkspaceProject, index: number): void {
  event.preventDefault();
  emit("drop", {
    iteration: props.group.iteration,
    beforeWorktreePath: rowAnchor(event, project, index),
  });
}

/** 表格空白区 / 表尾 → 追加到末尾 */
function onTableDragOver(event: DragEvent): void {
  if (!dropAcceptable.value) {
    return;
  }
  event.preventDefault();
  emit("dragOver", { iteration: props.group.iteration, beforeWorktreePath: null });
}

function onTableDrop(event: DragEvent): void {
  event.preventDefault();
  emit("drop", { iteration: props.group.iteration, beforeWorktreePath: null });
}

/** 021：整格复制；成功后该格显示「已复制」1.2 秒（重复点击重置计时） */
async function onCopy(text: string, key: string): Promise<void> {
  if (props.dragging || text.length === 0) {
    return;
  }
  try {
    await copyText(text);
  } catch (error) {
    ui?.toast(error instanceof Error ? error.message : "复制失败，请手动选择文本", "danger");
    return;
  }
  copiedKey.value = key;
  if (copyTimer !== null) {
    clearTimeout(copyTimer);
  }
  copyTimer = setTimeout(() => {
    copiedKey.value = null;
    copyTimer = null;
  }, 1200);
}

onBeforeUnmount(() => {
  if (copyTimer !== null) {
    clearTimeout(copyTimer);
  }
});

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

/** 合并单元格视图（无结论 → null；检查中 → 单独的「检查中…」标签） */
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
      <NoteTooltip
        v-if="props.group.note"
        :note="props.group.note"
        :id="`note-tip-${props.group.iteration}`"
      />
      <span class="iteration-path mono" :title="props.group.iterationPath">{{ props.group.iterationPath }}</span>
      <span class="spacer" />
      <div class="head-actions">
        <button type="button" class="btn" :disabled="!canEditNote" @click="startNoteEdit">✎ 备注</button>
        <button type="button" class="btn" :disabled="!manifestValid" @click="emit('hide', props.group.iteration)">隐藏</button>
        <button type="button" class="btn" :disabled="!manifestValid || props.archiving" @click="emit('archive', props.group.iteration)">
          {{ props.archiving ? "评估中…" : "归档" }}
        </button>
        <button type="button" class="btn" :disabled="!props.group.openable" @click="emit('openIteration', props.group.iteration)">
          打开目录
        </button>
        <button
          type="button"
          class="btn"
          :disabled="!manifestValid || props.checking"
          @click="emit('checkMerge', props.group.iteration)"
        >
          {{ props.checking ? MERGE_CHECKING_LABEL : "检查合并" }}
        </button>
      </div>
    </header>

    <div v-if="props.progressLine" class="progress-line">{{ props.progressLine }}</div>

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

      <div class="table-scroll" @dragover="onTableDragOver" @drop="onTableDrop">
        <table class="project-table">
          <thead>
            <tr>
              <th v-for="column in COLUMNS" v-show="column.locked || show(column.key)" :key="column.key">
                {{ column.label }}
              </th>
            </tr>
          </thead>
          <tbody>

            <tr
              v-for="(project, index) in props.group.projects"
              :key="`${project.projectId}-${project.worktreePath}`"
              :class="{
                'row-dragging': props.dragSourcePath === project.worktreePath,
                'drop-indicator': props.dropArmed && props.dropBeforePath === project.worktreePath,
              }"
              @dragover="onRowDragOver($event, project, index)"
              @drop="onRowDrop($event, project, index)"
            >
              <td v-show="show('project')">
                <span
                  v-if="project.lifecycle === 'active'"
                  class="drag-handle"
                  :class="{ disabled: props.filterActive }"
                  :draggable="!props.filterActive"
                  title="拖拽排序 / 跨迭代移动"
                  @dragstart="onHandleDragStart($event, project)"
                  @dragend="emit('dragEnd')"
                >⠿</span>
                <span>{{ project.projectId }}</span>
                <span v-if="project.renamedFrom" class="renamed-flag" :title="`原名 ${project.renamedFrom}`">
                  {{ RENAMED_LABEL }}
                </span>
              </td>
              <td v-show="show('branch')">
                <span v-if="copiedKey === `${project.worktreePath}:branch`" class="tag success">已复制</span>
                <span
                  v-else
                  class="cell-branch copyable"
                  :title="project.branchDisplay"
                  @click="onCopy(project.branchDisplay, `${project.worktreePath}:branch`)"
                >{{ project.branchDisplay }}</span>
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
                <span v-else-if="props.checking" class="tag neutral">{{ MERGE_CHECKING_LABEL }}</span>
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
                <span v-else-if="props.checking" class="tag neutral">{{ MERGE_CHECKING_LABEL }}</span>
                <span v-else class="tag neutral">{{ MERGE_NOT_CHECKED_LABEL }}</span>
              </td>
              <td v-show="show('baseCommit')">
                <span class="mono" :title="project.baseCommit ?? ''">{{ shortCommit(project.baseCommit) || "—" }}</span>
              </td>
              <td v-show="show('source')">
                <span class="cell-ellipsis" :title="project.sourceRepository">{{ project.sourceRepository || "—" }}</span>
              </td>
              <td v-show="show('worktreePath')">
                <span v-if="copiedKey === `${project.worktreePath}:worktreePath`" class="tag success">已复制</span>
                <span
                  v-else
                  class="cell-ellipsis mono copyable"
                  :title="project.worktreePath"
                  @click="onCopy(project.worktreePath, `${project.worktreePath}:worktreePath`)"
                >{{ project.worktreePath }}</span>
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
            <tr v-if="props.dropArmed && props.dropBeforePath === null && props.group.projects.length > 0" class="drop-indicator">
              <td :colspan="COLUMNS.length" />
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>


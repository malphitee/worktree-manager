<script setup lang="ts">
// 创建页（ui-spec.md §7.2）：迭代号、最近迭代胶囊、备注、统一分支名/基分支、项目勾选表、底部固定操作条。
// 纯前端状态联动：统一值覆盖已勾选行；新勾选行继承当前统一值；单行修改不反向影响统一值。
import { computed, ref } from "vue";
import type { AppConfig, CreateProjectRequest, CreateRequest } from "../types";
import { precheckIteration, precheckWorkspaceRoot } from "../utils/config";
import { validateNote } from "../utils/note";

interface ProjectRow {
  projectId: string;
  projectType: string;
  vendorAvailable: boolean;
  selected: boolean;
  branch: string;
  baseRef: string;
}

const props = defineProps<{ config: AppConfig }>();

const emit = defineEmits<{
  start: [request: CreateRequest];
  back: [];
  /** 013：按项目拉取基分支候选（S8 接线） */
  refreshBranches: [projectId: string];
  refreshAllBranches: [];
}>();

const iteration = ref("");
const note = ref("");
const unifiedBranch = ref("");
const unifiedBaseRef = ref("");

const rows = ref<ProjectRow[]>(
  props.config.projects.map((project) => ({
    projectId: project.id,
    projectType: project.projectType,
    vendorAvailable: project.vendorAvailable,
    selected: false,
    branch: "",
    baseRef: "",
  })),
);

const selectedRows = computed(() => rows.value.filter((row) => row.selected));

const iterationError = computed(() => (iteration.value.length === 0 ? "请填写迭代号" : precheckIteration(iteration.value)));
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

function applyUnifiedBranch(): void {
  for (const row of rows.value) {
    if (row.selected) {
      row.branch = unifiedBranch.value;
    }
  }
}

function applyUnifiedBaseRef(): void {
  for (const row of rows.value) {
    if (row.selected) {
      row.baseRef = unifiedBaseRef.value;
    }
  }
}

function toggleRow(row: ProjectRow): void {
  row.selected = !row.selected;
  if (row.selected) {
    row.branch = unifiedBranch.value;
    row.baseRef = unifiedBaseRef.value;
  }
}

function applyRecent(value: string): void {
  iteration.value = value;
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
    projects: selectedRows.value.map<CreateProjectRequest>((row) => ({
      projectId: row.projectId,
      branch: row.branch.trim().length > 0 ? row.branch.trim() : null,
      baseRef: row.baseRef,
    })),
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
          <input v-model="unifiedBranch" type="text" placeholder="如 feature/7.3.0" @input="applyUnifiedBranch" />
        </label>
        <label class="field" style="margin-top: 12px">
          统一基分支（留空 ＝ origin/master）
          <input v-model="unifiedBaseRef" type="text" placeholder="origin/master" @input="applyUnifiedBaseRef" />
        </label>
        <p class="hint">基分支为空时由后端按 origin/master 归一化；逐项目仍可单独修改。</p>
      </div>
    </div>

    <div class="card">
      <div class="page-head">
        <h2 style="margin: 0">项目</h2>
        <span class="spacer" />
        <button type="button" class="btn" title="逐项目串行拉取远端分支候选" @click="emit('refreshAllBranches')">⟳ 全部</button>
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
              <input type="checkbox" :checked="row.selected" :aria-label="`选择 ${row.projectId}`" @change="toggleRow(row)" />
            </td>
            <td>{{ row.projectId }}</td>
            <td>{{ row.projectType }}</td>
            <td>{{ row.vendorAvailable ? "有" : "无" }}</td>
            <td>
              <input v-model="row.branch" type="text" placeholder="留空 ＝ Detached" />
            </td>
            <td>
              <span class="field-row">
                <input v-model="row.baseRef" type="text" placeholder="origin/master" />
                <button
                  type="button"
                  class="icon-btn"
                  title="拉取该项目的远端分支候选"
                  @click="emit('refreshBranches', row.projectId)"
                >⟳</button>
              </span>
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


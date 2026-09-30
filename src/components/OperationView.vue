<script setup lang="ts">
// 创建进度视图（ui-spec.md §7.2）：每个项目一行，显示阶段标签与 message（含实际命令）。
import type { CreateBatchResult, CreateProgress } from "../types";
import { createPhaseLabel, createPhaseTone } from "../utils/status";

const props = defineProps<{
  items: CreateProgress[];
  running: boolean;
  result: CreateBatchResult | null;
  errorMessage: string | null;
}>();

const emit = defineEmits<{ back: [] }>();

const PHASE_TAG_LABELS = {
  statusCreated: "已创建",
  statusAlreadyExists: "已存在",
  statusFailed: "失败",
  statusAborted: "已终止",
} as const;

function resultLabel(): string | null {
  if (!props.result) {
    return null;
  }
  if (props.result.aborted) {
    return `${PHASE_TAG_LABELS.statusAborted}：${props.result.abortReason ?? ""}`;
  }
  const created = props.result.projects.filter((project) => project.status === "created").length;
  const existing = props.result.projects.filter((project) => project.status === "alreadyExists").length;
  const failed = props.result.projects.filter((project) => project.status === "failed").length;
  return `${PHASE_TAG_LABELS.statusCreated} ${created} 个 · ${PHASE_TAG_LABELS.statusAlreadyExists} ${existing} 个 · ${PHASE_TAG_LABELS.statusFailed} ${failed} 个`;
}
</script>

<template>
  <div>
    <div class="page-head">
      <h1>创建进度</h1>
      <span class="spacer" />
      <span v-if="props.running" class="hint">创建中…</span>
    </div>

    <div v-if="props.errorMessage" class="app-banner error">
      <span>{{ props.errorMessage }}</span>
    </div>

    <div class="card">
      <div class="progress-list">
        <div v-for="item in props.items" :key="`${item.index}-${item.projectId}`" class="progress-row">
          <span class="pid">{{ item.projectId }}</span>
          <span class="tag" :class="createPhaseTone(item.phase)">{{ createPhaseLabel(item.phase) }}</span>
          <span class="msg">{{ item.message }}</span>
        </div>
        <p v-if="props.items.length === 0 && !props.running" class="hint">没有进度信息。</p>
      </div>
    </div>

    <div v-if="resultLabel()" class="card">
      <p>{{ resultLabel() }}</p>
      <ul v-if="props.result && props.result.projects.length > 0" class="risk-list">
        <li v-for="project in props.result.projects" :key="project.projectId + project.worktreePath" class="risk-item">
          <span class="risk-title">
            {{ project.projectId }}
            <span class="tag" :class="project.status === 'failed' ? 'danger' : project.status === 'alreadyExists' ? 'warning' : 'success'">
              {{ project.status === 'created' ? PHASE_TAG_LABELS.statusCreated : project.status === 'alreadyExists' ? PHASE_TAG_LABELS.statusAlreadyExists : PHASE_TAG_LABELS.statusFailed }}
            </span>
          </span>
          <div class="risk-paths mono">{{ project.worktreePath }}</div>
          <div v-if="project.message" class="hint">{{ project.message }}</div>
        </li>
      </ul>
    </div>

    <div class="settings-actions">
      <button type="button" class="btn primary" :disabled="props.running" @click="emit('back')">返回列表</button>
    </div>
  </div>
</template>

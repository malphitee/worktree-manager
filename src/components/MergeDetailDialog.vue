<script setup lang="ts">
// 合并详情模态（001 / 006）：未合并 commit 列表与错误信息；Esc 关闭。
import { onMounted, ref } from "vue";
import type { MergeCellResult, WorkspaceProject } from "../types";
import type { MergeTarget } from "../utils/merge";
import { mergeCellLabel, mergeCellTone } from "../utils/merge";

const props = defineProps<{
  iteration: string;
  project: WorkspaceProject;
  target: MergeTarget;
  cell: MergeCellResult;
}>();

const emit = defineEmits<{ close: [] }>();

const panel = ref<HTMLElement | null>(null);
/** SSR（无 document）时 Teleport 不可用，降级为原地渲染 */
const canTeleport = typeof document !== "undefined";

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    emit("close");
  }
}

onMounted(() => {
  panel.value?.focus();
});
</script>

<template>
  <Teleport to="body" :disabled="!canTeleport">
    <div class="modal-backdrop" @click.self="emit('close')">
      <div ref="panel" class="modal-panel" tabindex="-1" @keydown="onKeydown">
        <div class="modal-head">
          合并详情 · {{ props.project.projectId }} → origin/{{ props.target }}
        </div>
        <div class="modal-body">
          <p>
            <span class="tag" :class="mergeCellTone(props.cell.status)">{{ mergeCellLabel(props.cell.status) }}</span>
            <span v-if="props.cell.stale" class="hint" style="margin-left: 8px">fetch 失败，使用本地 remote-tracking 缓存</span>
          </p>
          <p class="hint">
            worktree 路径：<span class="mono">{{ props.project.worktreePath }}</span><br />
            分支：<span class="mono">{{ props.project.branchDisplay }}</span>
          </p>
          <div v-if="props.cell.errorMessage" class="app-banner error" style="border-radius: 6px">
            <span>{{ props.cell.errorMessage }}</span>
          </div>
          <template v-if="props.cell.unmergedCommits.length > 0">
            <p style="margin-bottom: 4px">未合并的提交（{{ props.cell.unmergedCommits.length }}）：</p>
            <ul class="commit-list">
              <li v-for="commit in props.cell.unmergedCommits" :key="commit">{{ commit }}</li>
            </ul>
          </template>
          <p v-else-if="props.cell.status === 'unmerged'" class="hint">没有可列出的提交信息。</p>
        </div>
        <div class="modal-foot">
          <button type="button" class="btn primary" @click="emit('close')">关闭</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

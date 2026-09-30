<script setup lang="ts">
// 移除确认模态（ui-spec.md §6 / design 001-014 共用）：风险列表、vendor 清理、确认文本。
// 危险确认类模态：Esc 不关闭，必须点击按钮。
import { computed, onMounted, ref } from "vue";
import type { RemovalAssessment, WorkspaceProject } from "../types";
import { riskTone } from "../utils/status";

const props = defineProps<{
  iteration: string;
  project: WorkspaceProject;
  assessment: RemovalAssessment;
}>();

const emit = defineEmits<{
  confirm: [payload: { removeCopiedVendor: boolean; confirmation: string }];
  cancel: [];
}>();

const confirmation = ref("");
const removeCopiedVendor = ref(false);
const panel = ref<HTMLElement | null>(null);
/** SSR（无 document）时 Teleport 不可用，降级为原地渲染 */
const canTeleport = typeof document !== "undefined";

const textMatches = computed(() => confirmation.value === props.assessment.confirmationText);
const canConfirm = computed(() => props.assessment.allowed && textMatches.value);

onMounted(() => {
  panel.value?.focus();
});
</script>

<template>
  <Teleport to="body" :disabled="!canTeleport">
    <div class="modal-backdrop">
      <div ref="panel" class="modal-panel" tabindex="-1">
        <div class="modal-head">移除 worktree · {{ props.project.projectId }}</div>
        <div class="modal-body">
          <p class="hint">worktree 路径：<span class="mono">{{ props.assessment.worktreePath }}</span></p>
          <p v-if="props.assessment.allowed" class="hint">
            后端未发现阻止移除的风险；移除不会删除本地分支，也不会 stash / commit / push。
          </p>
          <ul v-if="props.assessment.risks.length > 0" class="risk-list">
            <li
              v-for="risk in props.assessment.risks"
              :key="risk.code"
              class="risk-item"
              :class="{ blocking: risk.severity === 'blocking' }"
            >
              <div class="risk-title">
                <span class="tag" :class="riskTone(risk.severity)">{{ risk.severity === "blocking" ? "阻止" : "警告" }}</span>
                <span>{{ risk.message }}</span>
              </div>
              <div v-if="risk.paths.length > 0" class="risk-paths mono">
                {{ risk.paths.join("、") }}
              </div>
            </li>
          </ul>

          <label v-if="props.assessment.vendorOnlyCleanupAvailable" class="field" style="margin-top: 12px">
            <span>
              <input v-model="removeCopiedVendor" type="checkbox" />
              先删除本工具复制的 vendor（仅 vendor 为未跟踪内容时可用）
            </span>
          </label>

          <label class="field" style="margin-top: 12px">
            输入 <span class="mono">{{ props.assessment.confirmationText }}</span> 以确认
            <input v-model="confirmation" type="text" class="mono" :placeholder="props.assessment.confirmationText" />
          </label>
        </div>
        <div class="modal-foot">
          <button type="button" class="btn" @click="emit('cancel')">取消</button>
          <button
            type="button"
            class="btn danger"
            :disabled="!canConfirm"
            @click="emit('confirm', { removeCopiedVendor, confirmation })"
          >移除</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

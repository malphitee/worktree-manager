<script setup lang="ts">
// 归档模态（008 / ui-spec.md §6）：干净 / 强制两态；危险确认类模态，Esc 不关闭。
import { computed, onMounted, ref } from "vue";
import type { ArchiveAssessment } from "../types";
import { mergeCellLabel, mergeCellTone } from "../utils/merge";

const props = defineProps<{
  iteration: string;
  assessment: ArchiveAssessment;
}>();

const emit = defineEmits<{
  confirm: [payload: { force: boolean }];
  cancel: [];
}>();

const confirmation = ref("");
const force = ref(false);
const panel = ref<HTMLElement | null>(null);
/** SSR（无 document）时 Teleport 不可用，降级为原地渲染 */
const canTeleport = typeof document !== "undefined";

const dirtyRecords = computed(() => props.assessment.records.filter((record) => !record.clean));
const textMatches = computed(() => confirmation.value === props.assessment.confirmationText);
const canConfirm = computed(() => (props.assessment.clean ? true : force.value && textMatches.value));

onMounted(() => {
  panel.value?.focus();
});
</script>

<template>
  <Teleport to="body" :disabled="!canTeleport">
    <div class="modal-backdrop">
      <div ref="panel" class="modal-panel" tabindex="-1">
        <div class="modal-head">归档迭代 · {{ props.iteration }}</div>
        <div class="modal-body">
          <p v-if="props.assessment.clean" class="hint">
            全部记录均已合并进 develop / master 且工作区干净；归档会移除全部 worktree，保留本地分支、迭代目录与公共目录。
          </p>
          <template v-else>
            <p class="hint">
              以下记录不满足干净条件。强制归档会使用 <span class="mono">worktree remove --force</span>，
              未提交改动与未推送提交将无法恢复。
            </p>
            <ul class="risk-list">
              <li v-for="record in dirtyRecords" :key="record.projectId + record.worktreePath" class="risk-item blocking">
                <div class="risk-title">
                  <span class="tag danger">不干净</span>
                  <span>{{ record.projectId }}</span>
                  <span class="tag" :class="mergeCellTone(record.develop.status)">develop {{ mergeCellLabel(record.develop.status) }}</span>
                  <span class="tag" :class="mergeCellTone(record.master.status)">master {{ mergeCellLabel(record.master.status) }}</span>
                </div>
                <div v-if="record.blockers.length > 0" class="risk-paths">{{ record.blockers.join("；") }}</div>
              </li>
            </ul>
            <label class="field" style="margin-top: 12px">
              <span><input v-model="force" type="checkbox" /> 我了解风险，强制归档不干净的记录</span>
            </label>
            <label class="field" style="margin-top: 12px">
              输入 <span class="mono">{{ props.assessment.confirmationText }}</span> 以确认
              <input v-model="confirmation" type="text" class="mono" :placeholder="props.assessment.confirmationText" />
            </label>
          </template>
        </div>
        <div class="modal-foot">
          <button type="button" class="btn" @click="emit('cancel')">取消</button>
          <button
            type="button"
            class="btn danger"
            :disabled="!canConfirm"
            @click="emit('confirm', { force: !props.assessment.clean && force })"
          >
            {{ props.assessment.clean ? "归档" : "强制归档" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

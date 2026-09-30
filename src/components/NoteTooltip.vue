<script setup lang="ts">
// 备注气泡（设计 012 / ui-spec.md §7.4）：Teleport 到 body，按触发元素位置定位，
// scroll / resize（capture: true）立即收起；组件卸载时移除监听。
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = defineProps<{ text: string; anchor: HTMLElement | null }>();

/** SSR（无 document）时 Teleport 不可用，降级为原地渲染 */
const canTeleport = typeof document !== "undefined";
const position = ref<{ top: number; left: number } | null>(null);

const GAP = 6;
const MAX_WIDTH = 320;
const ESTIMATED_HEIGHT = 40;

function updatePosition(): void {
  const el = props.anchor;
  if (!el || typeof window === "undefined") {
    position.value = null;
    return;
  }
  const rect = el.getBoundingClientRect();
  const below = rect.bottom + GAP;
  const flip = below + ESTIMATED_HEIGHT > window.innerHeight;
  const maxLeft = Math.max(8, window.innerWidth - MAX_WIDTH - 8);
  position.value = {
    top: flip ? Math.max(8, rect.top - GAP - ESTIMATED_HEIGHT) : below,
    left: Math.min(Math.max(8, rect.left), maxLeft),
  };
}

function collapse(): void {
  position.value = null;
}

watch(() => props.anchor, updatePosition);
watch(() => props.text, updatePosition);

onMounted(() => {
  updatePosition();
  window.addEventListener("scroll", collapse, true);
  window.addEventListener("resize", collapse, true);
});

onBeforeUnmount(() => {
  window.removeEventListener("scroll", collapse, true);
  window.removeEventListener("resize", collapse, true);
});
</script>

<template>
  <Teleport to="body" :disabled="!canTeleport">
    <div
      v-if="position"
      class="note-tooltip"
      role="tooltip"
      :style="{ top: `${position.top}px`, left: `${position.left}px` }"
    >
      {{ props.text }}
    </div>
  </Teleport>
</template>

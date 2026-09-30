<script setup lang="ts">
// 备注气泡（设计 012）：标记 `▤` + `<Teleport to="body">` 的 fixed 气泡。
// hover 与 focus 都显示；`scroll` / `resize`（capture: true）立即收起；卸载时移除监听。
import { onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  note: string;
  /** aria-describedby 目标 id，建议 `note-tip-${iteration}` */
  id: string;
}>();

/** SSR（无 document）时 Teleport 不可用，降级为原地渲染 */
const canTeleport = typeof document !== "undefined";
const open = ref(false);
const anchor = ref<HTMLElement | null>(null);
const position = ref<{ top: number; left: number } | null>(null);

const GAP = 6;
const MAX_WIDTH = 320;
const ESTIMATED_HEIGHT = 40;

function show(): void {
  updatePosition();
  open.value = true;
}

function hide(): void {
  open.value = false;
}

function collapse(): void {
  // 滚动 / 尺寸变化：立即收起，避免气泡与触发元素错位
  open.value = false;
}

function updatePosition(): void {
  const element = anchor.value;
  if (!element || typeof window === "undefined") {
    return;
  }
  const rect = element.getBoundingClientRect();
  const below = rect.bottom + GAP;
  const flip = below + ESTIMATED_HEIGHT > window.innerHeight;
  const maxLeft = Math.max(8, window.innerWidth - MAX_WIDTH - 8);
  position.value = {
    top: flip ? Math.max(8, rect.top - GAP - ESTIMATED_HEIGHT) : below,
    left: Math.min(Math.max(8, rect.left), maxLeft),
  };
}

onMounted(() => {
  window.addEventListener("scroll", collapse, true);
  window.addEventListener("resize", collapse, true);
});

onBeforeUnmount(() => {
  window.removeEventListener("scroll", collapse, true);
  window.removeEventListener("resize", collapse, true);
});
</script>

<template>
  <span
    ref="anchor"
    class="note-mark"
    tabindex="0"
    :aria-describedby="open ? props.id : undefined"
    @mouseenter="show"
    @mouseleave="hide"
    @focus="show"
    @blur="hide"
  >▤</span>
  <Teleport to="body" :disabled="!canTeleport">
    <div
      v-if="open && position"
      :id="props.id"
      role="tooltip"
      class="note-tooltip"
      :style="{ top: `${position.top}px`, left: `${position.left}px` }"
    >{{ props.note }}</div>
  </Teleport>
</template>

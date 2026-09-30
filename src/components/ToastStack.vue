<script setup lang="ts">
// 右上角 toast（ui-spec.md §2）：三态着色，4 秒自动消失，hover 暂停。
import { onUnmounted, ref, watch } from "vue";

interface ToastLike {
  id: number;
  tone: "success" | "warning" | "danger";
  message: string;
}

const props = defineProps<{ toasts: ToastLike[] }>();
const emit = defineEmits<{ dismiss: [id: number] }>();

const AUTO_DISMISS_MS = 4000;

const timers = new Map<number, ReturnType<typeof setTimeout>>();
const remaining = new Map<number, number>();
const startedAt = new Map<number, number>();
const paused = ref<number[]>([]);

function clearTimer(id: number): void {
  const timer = timers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

function schedule(toast: ToastLike, ms: number): void {
  // SSR / 非浏览器环境不启动计时器
  if (typeof window === "undefined") {
    return;
  }
  clearTimer(toast.id);
  startedAt.set(toast.id, Date.now());
  remaining.set(toast.id, ms);
  timers.set(
    toast.id,
    setTimeout(() => {
      timers.delete(toast.id);
      emit("dismiss", toast.id);
    }, ms),
  );
}

function pause(id: number): void {
  const started = startedAt.get(id);
  if (started === undefined || !timers.has(id)) {
    return;
  }
  const left = Math.max(0, (remaining.get(id) ?? AUTO_DISMISS_MS) - (Date.now() - started));
  remaining.set(id, left);
  clearTimer(id);
  paused.value = [...paused.value, id];
}

function resume(id: number): void {
  paused.value = paused.value.filter((item) => item !== id);
  const toast = props.toasts.find((item) => item.id === id);
  if (toast) {
    schedule(toast, Math.max(remaining.get(id) ?? AUTO_DISMISS_MS, 200));
  }
}

watch(
  () => props.toasts.map((toast) => toast.id),
  (ids) => {
    for (const id of ids) {
      if (!timers.has(id) && !paused.value.includes(id)) {
        const toast = props.toasts.find((item) => item.id === id);
        if (toast) {
          schedule(toast, AUTO_DISMISS_MS);
        }
      }
    }
    for (const id of [...timers.keys()]) {
      if (!ids.includes(id)) {
        clearTimer(id);
      }
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  for (const timer of timers.values()) {
    clearTimeout(timer);
  }
  timers.clear();
});
</script>

<template>
  <div class="toast-stack" aria-live="polite">
    <div
      v-for="toast in props.toasts"
      :key="toast.id"
      class="toast"
      :class="toast.tone"
      @mouseenter="pause(toast.id)"
      @mouseleave="resume(toast.id)"
    >
      <span>{{ toast.message }}</span>
      <button type="button" class="icon-btn" aria-label="关闭" @click="emit('dismiss', toast.id)">×</button>
    </div>
  </div>
</template>

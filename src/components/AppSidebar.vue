<script setup lang="ts">
// 侧栏导航：工作区 / 设置（ui-spec.md §2）
import type { PageKey } from "../types";

const props = defineProps<{ page: PageKey }>();
const emit = defineEmits<{ navigate: [page: PageKey] }>();

const NAV_ITEMS: { key: PageKey; label: string }[] = [
  { key: "workspaces", label: "工作区" },
  { key: "settings", label: "设置" },
];
</script>

<template>
  <aside class="app-sidebar">
    <div class="sidebar-brand">Worktree Manager</div>
    <nav class="sidebar-nav">
      <button
        v-for="item in NAV_ITEMS"
        :key="item.key"
        type="button"
        class="nav-item"
        :class="{ active: props.page === item.key || (item.key === 'workspaces' && props.page === 'create') }"
        @click="emit('navigate', item.key)"
      >
        {{ item.label }}
      </button>
    </nav>
  </aside>
</template>

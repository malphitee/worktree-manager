<script setup lang="ts">
// 设置页（ui-spec.md §7.3）：工作区根目录、公共目录规则表、项目表。
// 注意：从列表移除项目只改配置，不动磁盘仓库；目录选择走 dialog 插件 + resolve_projects。
import { computed, inject, ref, watch } from "vue";
import * as api from "../api/tauri";
import type { AppConfig, ProjectConfig, SharedDirectoryRule } from "../types";
import { UI_CONTEXT_KEY } from "../utils/context";
import { cloneConfig, precheckSingleSegmentName } from "../utils/config";

const props = defineProps<{ config: AppConfig }>();

const emit = defineEmits<{
  save: [config: AppConfig];
}>();

const ui = inject(UI_CONTEXT_KEY, null);

const form = ref<AppConfig>(cloneConfig(props.config));

watch(
  () => props.config,
  (next) => {
    form.value = cloneConfig(next);
  },
);

const formError = computed<string | null>(() => {
  const targetDirs = new Set<string>();
  for (const rule of form.value.sharedDirectories) {
    const nameError = precheckSingleSegmentName(rule.targetDirectory);
    if (nameError) {
      return `公共目录目标名「${rule.targetDirectory}」不合法：${nameError}`;
    }
    const key = rule.targetDirectory.toLowerCase();
    if (targetDirs.has(key)) {
      return `公共目录目标名重复：${rule.targetDirectory}`;
    }
    targetDirs.add(key);
    if (rule.sourcePath.trim().length === 0) {
      return "公共目录源目录不能为空";
    }
  }
  const projectIds = new Set<string>();
  for (const project of form.value.projects) {
    const idError = precheckSingleSegmentName(project.id);
    if (idError) {
      return `项目标识「${project.id}」不合法：${idError}`;
    }
    const key = project.id.toLowerCase();
    if (projectIds.has(key)) {
      return `项目标识重复：${project.id}`;
    }
    if (targetDirs.has(key)) {
      return `项目标识与公共目录目标名冲突：${project.id}`;
    }
    projectIds.add(key);
  }
  return null;
});

function addRule(): void {
  form.value.sharedDirectories.push({ sourcePath: "", targetDirectory: "" });
}

function removeRule(index: number): void {
  form.value.sharedDirectories.splice(index, 1);
}

function removeProject(index: number): void {
  form.value.projects.splice(index, 1);
}

function save(): void {
  if (formError.value) {
    return;
  }
  emit("save", cloneConfig(form.value));
}

/** 选择工作区根目录（系统目录对话框；浏览器演示模式返回空数组） */
async function chooseRoot(): Promise<void> {
  try {
    const paths = await api.pickDirectories(false);
    if (paths.length > 0) {
      form.value.workspaceRoot = paths[0] ?? null;
    }
  } catch (err) {
    ui?.toast(api.toAppError(err).message, "danger");
  }
}

/** 目录多选 → resolve_projects：逐条显示解析结果或错误 */
async function addProjects(): Promise<void> {
  try {
    const paths = await api.pickDirectories(true);
    if (paths.length === 0) {
      return;
    }
    const resolutions = await api.resolveProjects(paths);
    for (const resolution of resolutions) {
      if (resolution.repositoryPath && resolution.suggestedId) {
        form.value.projects.push({
          id: resolution.suggestedId,
          repositoryPath: resolution.repositoryPath,
          projectType: resolution.projectType,
          vendorAvailable: resolution.vendorAvailable,
        });
      } else {
        ui?.toast(
          `${resolution.inputPath}：${resolution.error ?? "无法解析为 Git 仓库"}`,
          "danger",
        );
      }
    }
  } catch (err) {
    ui?.toast(api.toAppError(err).message, "danger");
  }
}

function projectTypeLabel(type: ProjectConfig["projectType"]): string {
  switch (type) {
    case "php":
      return "PHP";
    case "go":
      return "Go";
    case "other":
      return "其他";
    case "unknown":
      return "未知";
  }
}

const rules = computed<SharedDirectoryRule[]>(() => form.value.sharedDirectories);
const projects = computed<ProjectConfig[]>(() => form.value.projects);
</script>

<template>
  <div>
    <div class="page-head">
      <h1>设置</h1>
      <span class="spacer" />
      <span v-if="formError" class="field-error">{{ formError }}</span>
      <button type="button" class="btn primary" :disabled="formError !== null" @click="save">保存</button>
    </div>

    <div class="settings-grid">
      <div class="card">
        <h2>工作区根目录<span class="required-mark">*</span></h2>
        <div class="field-row">
          <input v-model="form.workspaceRoot" class="mono wide" type="text" placeholder="如 /Users/me/Work/workspace" />
          <button type="button" class="btn" @click="chooseRoot">选择目录</button>
        </div>
        <p class="hint">所有迭代目录创建在该目录下；允许目录尚不存在，首次创建时自动建立。</p>
      </div>
    </div>

    <div class="card">
      <div class="page-head">
        <h2 style="margin: 0">公共目录规则</h2>
        <span class="spacer" />
        <button type="button" class="btn" @click="addRule">添加规则</button>
      </div>
      <table class="rule-table">
        <thead>
          <tr>
            <th>源目录</th>
            <th style="width: 220px">迭代内目标目录名</th>
            <th style="width: 60px" />
          </tr>
        </thead>
        <tbody>
          <tr v-for="(rule, index) in rules" :key="index">
            <td><input v-model="rule.sourcePath" class="mono" type="text" placeholder="/path/to/source" /></td>
            <td><input v-model="rule.targetDirectory" type="text" placeholder="fd-common" /></td>
            <td>
              <button type="button" class="icon-btn danger" aria-label="删除规则" @click="removeRule(index)">×</button>
            </td>
          </tr>
          <tr v-if="rules.length === 0">
            <td colspan="3" class="hint">还没有公共目录规则；没有规则时不复制任何公共目录。</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="card">
      <div class="page-head">
        <h2 style="margin: 0">项目</h2>
        <span class="spacer" />
        <button type="button" class="btn" @click="addProjects">添加项目</button>
      </div>
      <table class="project-config-table">
        <thead>
          <tr>
            <th style="width: 180px">标识</th>
            <th>仓库路径</th>
            <th style="width: 90px">类型</th>
            <th style="width: 110px">vendor</th>
            <th style="width: 60px" />
          </tr>
        </thead>
        <tbody>
          <tr v-for="(project, index) in projects" :key="index">
            <td><input v-model="project.id" type="text" /></td>
            <td><input v-model="project.repositoryPath" class="mono" type="text" /></td>
            <td>{{ projectTypeLabel(project.projectType) }}</td>
            <td>{{ project.vendorAvailable ? "源仓库有 vendor" : "无" }}</td>
            <td>
              <button type="button" class="icon-btn danger" aria-label="移除项目" @click="removeProject(index)">×</button>
            </td>
          </tr>
          <tr v-if="projects.length === 0">
            <td colspan="5" class="hint">还没有项目；点击「添加项目」选择仓库目录。</td>
          </tr>
        </tbody>
      </table>
      <p class="hint">从列表移除项目只修改配置，不会删除磁盘上的仓库或 worktree。</p>
    </div>
  </div>
</template>


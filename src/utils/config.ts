// 设置表单 ↔ AppConfig 转换与前端格式预检（后端仍会重新校验全部输入，见 architecture.md §4.3）
import type { AppConfig } from "../types";

const ILLEGAL_CHARS = new Set(["/", "\\", ":", "*", "?", "\"", "<", ">", "|"]);

const RESERVED_DEVICE_NAMES = new Set([
  "CON", "PRN", "AUX", "NUL",
  "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
  "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
]);

/** 深拷贝配置（表单编辑用，避免直接改动 App.vue 持有的状态） */
export function cloneConfig(config: AppConfig): AppConfig {
  return JSON.parse(JSON.stringify(config)) as AppConfig;
}

/** 单层目录名 / 项目标识预检（与 validation.rs 的跨平台统一规则一致）；返回错误文案或 null */
export function precheckSingleSegmentName(raw: string): string | null {
  if (raw.length === 0) {
    return "不能为空";
  }
  if (raw === "." || raw === "..") {
    return "不能使用 . 或 ..";
  }
  for (const ch of raw) {
    if (ILLEGAL_CHARS.has(ch)) {
      return `不能包含字符 ${ch}`;
    }
    const code = ch.codePointAt(0) ?? 0;
    if (code < 32 || code === 127) {
      return "不能包含控制字符";
    }
  }
  if (raw.endsWith(" ")) {
    return "末尾不能有空格";
  }
  if (raw.endsWith(".")) {
    return "末尾不能是句点";
  }
  const base = (raw.split(".")[0] ?? "").toUpperCase();
  if (RESERVED_DEVICE_NAMES.has(base)) {
    return `不能使用保留设备名 ${base}`;
  }
  return null;
}

/** 迭代号预检（创建页禁用条件之一） */
export function precheckIteration(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return "请填写迭代号";
  }
  if (trimmed !== raw) {
    return "迭代号首尾不能有空白";
  }
  return precheckSingleSegmentName(raw);
}

/** 工作区根目录预检（必填；允许尚不存在，但必须是绝对路径） */
export function precheckWorkspaceRoot(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    // 与后端 create_workspaces 的校验文案保持一致（验收 §1/§2）
    return "请先在设置中填写工作区根目录";
  }
  if (!looksAbsolute(trimmed)) {
    return "必须是绝对路径";
  }
  return null;
}

function looksAbsolute(path: string): boolean {
  if (path.startsWith("/")) {
    return true;
  }
  if (/^[A-Za-z]:[\\/]/.test(path)) {
    return true;
  }
  return path.startsWith("\\\\");
}

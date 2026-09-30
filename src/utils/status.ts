// 状态 → 标签文案 / 色调映射（来源：ui-spec.md §8）
// 约定：组件内不写死文案分支，全部经本模块。
import type { CreatePhase, SharedDirStatus, Validity, VendorStatus } from "../types";

export type Tone = "neutral" | "success" | "warning" | "danger";

export function validityLabel(v: Validity): string {
  switch (v) {
    case "valid":
      return "有效";
    case "unknown":
      return "未复核";
    case "missingDirectory":
      return "目录缺失";
    case "notRegistered":
      return "未注册";
    case "headMismatch":
      return "HEAD 不一致";
    case "sourceMissing":
      return "源仓库缺失";
    case "removed":
      return "已移除";
    case "discovered":
      return "未托管";
  }
}

export function validityTone(v: Validity): Tone {
  switch (v) {
    case "valid":
      return "success";
    case "unknown":
      return "neutral";
    case "missingDirectory":
      return "danger";
    case "notRegistered":
      return "danger";
    case "headMismatch":
      return "warning";
    case "sourceMissing":
      return "danger";
    case "removed":
      return "neutral";
    case "discovered":
      return "warning";
  }
}

export function vendorLabel(v: VendorStatus): string {
  switch (v) {
    case "copied":
      return "已复制";
    case "notPhp":
      return "无需复制";
    case "sourceMissing":
      return "源无 vendor";
    case "lockMissing":
      return "缺少 lock";
    case "lockMismatch":
      return "lock 不一致";
    case "targetExists":
      return "目标已存在";
    case "copyFailed":
      return "复制失败";
  }
}

export function vendorTone(v: VendorStatus): Tone {
  switch (v) {
    case "copied":
      return "success";
    case "notPhp":
      return "neutral";
    case "sourceMissing":
      return "neutral";
    case "lockMissing":
      return "warning";
    case "lockMismatch":
      return "warning";
    case "targetExists":
      return "neutral";
    case "copyFailed":
      return "danger";
  }
}

export function sharedDirLabel(s: SharedDirStatus): string {
  switch (s) {
    case "pending":
      return "待复制";
    case "reused":
      return "已复用";
    case "copied":
      return "已复制";
    case "failed":
      return "失败";
    case "notRequired":
      return "无需复制";
  }
}

export function sharedDirTone(s: SharedDirStatus): Tone {
  switch (s) {
    case "pending":
      return "neutral";
    case "reused":
      return "neutral";
    case "copied":
      return "success";
    case "failed":
      return "danger";
    case "notRequired":
      return "neutral";
  }
}

/** 复核结果中 `renamedFrom` 非空的行数（用于「已同步 N 个分支重命名」toast，设计 011 §3.3） */
export function countRenamed(groups: { projects: { renamedFrom: string | null }[] }[]): number {
  return groups.reduce(
    (total, group) => total + group.projects.filter((project) => project.renamedFrom !== null).length,
    0,
  );
}

export function createPhaseLabel(p: CreatePhase): string {
  switch (p) {
    case "queued":
      return "排队中";
    case "fetching":
      return "拉取中";
    case "creating":
      return "创建中";
    case "vendor":
      return "vendor";
    case "completed":
      return "完成";
    case "failed":
      return "失败";
  }
}

export function createPhaseTone(p: CreatePhase): Tone {
  switch (p) {
    case "queued":
      return "neutral";
    case "fetching":
      return "neutral";
    case "creating":
      return "neutral";
    case "vendor":
      return "neutral";
    case "completed":
      return "success";
    case "failed":
      return "danger";
  }
}

// 基准变动 / 未提交 文案（ui-spec.md §4.1「基准变动」列）
export const CHANGES_LABELS = {
  hasChanges: "有变更",
  noChanges: "无",
  unreconciled: "未复核",
  dirty: "未提交",
} as const;

export function hasChangesLabel(hasChanges: boolean | null): string {
  if (hasChanges === null) {
    return CHANGES_LABELS.unreconciled;
  }
  return hasChanges ? CHANGES_LABELS.hasChanges : CHANGES_LABELS.noChanges;
}

export function hasChangesTone(hasChanges: boolean | null): Tone {
  if (hasChanges === null) {
    return "neutral";
  }
  return hasChanges ? "warning" : "neutral";
}

/** 011 改名同步标记（ui-spec.md §4.1「项目」列） */
export const RENAMED_LABEL = "⟳ 已同步改名";

/** 合并检查 stale 标记 */
export const STALE_LABEL = "可能过时";

/** 移除风险等级 → 色调 */
export function riskTone(severity: "warning" | "blocking"): Tone {
  return severity === "blocking" ? "danger" : "warning";
}

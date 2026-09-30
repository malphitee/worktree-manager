// 合并检查结论 → 标签文案 / 色调映射（ui-spec.md §8）
import type { MergeCellStatus } from "../types";
import type { Tone } from "./status";

export function mergeCellLabel(s: MergeCellStatus): string {
  switch (s) {
    case "merged":
      return "已合并";
    case "contained":
      return "已包含（疑似 squash）";
    case "unmerged":
      return "未合并";
    case "targetMissing":
      return "目标分支不存在";
    case "branchMissing":
      return "分支不存在";
    case "notCheckable":
      return "无法检查";
    case "error":
      return "检查出错";
  }
}

export function mergeCellTone(s: MergeCellStatus): Tone {
  switch (s) {
    case "merged":
      return "success";
    case "contained":
      return "success";
    case "unmerged":
      return "warning";
    case "targetMissing":
      return "neutral";
    case "branchMissing":
      return "danger";
    case "notCheckable":
      return "neutral";
    case "error":
      return "danger";
  }
}

/** 尚未执行合并检查时的单元格文案（无后端结论，前端不推导） */
export const MERGE_NOT_CHECKED_LABEL = "未检查";

/** 合并检查目标分支（固定两个，不可配置） */
export const MERGE_TARGETS = ["develop", "master"] as const;
export type MergeTarget = (typeof MERGE_TARGETS)[number];

// 创建页前端状态联动（ui-spec.md §7.2）与请求组装。
// 纯函数，便于 SSR 测试直接断言：统一值覆盖已勾选行；新勾选行继承当前统一值；单行修改不反向影响统一值。
import type { CreateProjectRequest, ProjectConfig } from "../types";

export interface CreateProjectRow {
  projectId: string;
  projectType: string;
  vendorAvailable: boolean;
  selected: boolean;
  branch: string;
  baseRef: string;
}

/** 由配置项目构建初始行（全部未勾选、分支与基分支为空） */
export function buildRows(projects: ProjectConfig[]): CreateProjectRow[] {
  return projects.map((project) => ({
    projectId: project.id,
    projectType: project.projectType,
    vendorAvailable: project.vendorAvailable,
    selected: false,
    branch: "",
    baseRef: "",
  }));
}

/** 修改统一分支名 → 覆盖所有已勾选行 */
export function applyUnifiedBranch(rows: CreateProjectRow[], unifiedBranch: string): void {
  for (const row of rows) {
    if (row.selected) {
      row.branch = unifiedBranch;
    }
  }
}

/** 修改统一基分支 → 覆盖所有已勾选行 */
export function applyUnifiedBaseRef(rows: CreateProjectRow[], unifiedBaseRef: string): void {
  for (const row of rows) {
    if (row.selected) {
      row.baseRef = unifiedBaseRef;
    }
  }
}

/** 勾选 / 取消勾选：新勾选的行继承当前统一值；取消勾选不改动其他行 */
export function toggleRowSelection(
  row: CreateProjectRow,
  unifiedBranch: string,
  unifiedBaseRef: string,
): void {
  row.selected = !row.selected;
  if (row.selected) {
    row.branch = unifiedBranch;
    row.baseRef = unifiedBaseRef;
  }
}

/** 组装逐项目请求：空白分支名 → null（Detached HEAD）；基分支空串交给后端归一化为 origin/master */
export function toProjectRequests(rows: CreateProjectRow[]): CreateProjectRequest[] {
  return rows
    .filter((row) => row.selected)
    .map((row) => ({
      projectId: row.projectId,
      branch: row.branch.trim().length > 0 ? row.branch.trim() : null,
      baseRef: row.baseRef,
    }));
}

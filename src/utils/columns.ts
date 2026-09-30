// 列表列定义（ui-spec.md §4.1；顺序固定，可勾选显隐，恒显列不可取消）

export interface ColumnDef {
  key: string;
  label: string;
  /** 恒显列 */
  locked: boolean;
}

export const COLUMNS: ColumnDef[] = [
  { key: "project", label: "项目", locked: true },
  { key: "branch", label: "分支 / HEAD", locked: false },
  { key: "dirty", label: "基准变动", locked: false },
  { key: "mergeDevelop", label: "develop", locked: false },
  { key: "mergeMaster", label: "master", locked: false },
  { key: "baseCommit", label: "基准 Commit", locked: false },
  { key: "source", label: "源仓库", locked: false },
  { key: "worktreePath", label: "Worktree 路径", locked: false },
  { key: "vendor", label: "vendor", locked: false },
  { key: "createdAt", label: "创建时间", locked: false },
  { key: "status", label: "状态", locked: true },
  { key: "actions", label: "操作", locked: true },
];

export const LOCKED_COLUMN_KEYS: string[] = COLUMNS.filter((c) => c.locked).map((c) => c.key);

/** 合并已存储的可见列与恒显列；未知 key 丢弃；恒显列始终包含 */
export function resolveVisibleColumns(stored: string[] | null): string[] {
  const known = new Set(COLUMNS.map((c) => c.key));
  const base = stored === null ? COLUMNS.map((c) => c.key) : stored.filter((key) => known.has(key));
  const result = new Set(base);
  for (const key of LOCKED_COLUMN_KEYS) {
    result.add(key);
  }
  return COLUMNS.map((c) => c.key).filter((key) => result.has(key));
}

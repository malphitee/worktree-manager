// localStorage 读写（只允许 ui-spec.md §9 列出的三个键）
// 读取失败（JSON 损坏）时回退默认值并覆盖写入；SSR / 无 localStorage 时返回默认值。

const KEY_VISIBLE_COLUMNS = "worktree-manager.visible-columns";
const KEY_EXPANDED_ITERATIONS = "worktree-manager.expanded-iterations";
const KEY_HIDDEN_ZONE_EXPANDED = "worktree-manager.hidden-zone-expanded";

function storage(): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function readStringArray(key: string): string[] | null {
  const store = storage();
  if (!store) {
    return null;
  }
  const raw = store.getItem(key);
  if (raw === null) {
    return null;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((item) => typeof item === "string")) {
      return parsed;
    }
  } catch {
    // 损坏 → 落入默认值并覆盖写入
  }
  store.setItem(key, "[]");
  return null;
}

function writeStringArray(key: string, value: string[]): void {
  const store = storage();
  if (!store) {
    return;
  }
  store.setItem(key, JSON.stringify(value));
}

/** 可见列；null ＝ 键不存在，使用「全部可见」默认值 */
export function readVisibleColumns(): string[] | null {
  return readStringArray(KEY_VISIBLE_COLUMNS);
}

export function writeVisibleColumns(keys: string[]): void {
  writeStringArray(KEY_VISIBLE_COLUMNS, keys);
}

/** 展开的迭代号；null ＝ 键不存在，使用「全部展开」默认值 */
export function readExpandedIterations(): string[] | null {
  return readStringArray(KEY_EXPANDED_ITERATIONS);
}

export function writeExpandedIterations(iterations: string[]): void {
  writeStringArray(KEY_EXPANDED_ITERATIONS, iterations);
}

/** 已隐藏迭代收纳区是否展开；默认折叠 */
export function readHiddenZoneExpanded(): boolean {
  const store = storage();
  if (!store) {
    return false;
  }
  return store.getItem(KEY_HIDDEN_ZONE_EXPANDED) === "1";
}

export function writeHiddenZoneExpanded(expanded: boolean): void {
  const store = storage();
  if (!store) {
    return;
  }
  store.setItem(KEY_HIDDEN_ZONE_EXPANDED, expanded ? "1" : "0");
}

# 004 · 手动复核（快扫 / 复核两档）

> 设计文档是**当次决策的历史记录**，设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 相关基线：`docs/requirements.md §3.5 §3.11`、`docs/data-model.md §3 §5`、`docs/architecture.md §6.2 §7.1`、`docs/ui-spec.md §7.1`、`docs/design/011-branch-rename-sync/design.md`。

---

## 1. 背景与目标

### 现状

若列表每次加载都跑 `git worktree list` / `git status`，20 个迭代 × 15 个项目意味着数百次 Git 子进程，启动与每次保存设置后都要等数秒；同时复核可能回写清单（011），隐式触发会让用户在不知情时改动文件。

### 目标

- 列表明确分两档：**快扫**（只读目录与清单，不跑 Git，毫秒级）与**复核**（跑 Git、扫 discovered、可能回写清单）。
- 复核只由页面标题右侧唯一的「复核状态」按钮触发。
- 复核中旧列表保留、按钮 loading；完成后整体替换。
- 不提供刷新按钮、不做定时轮询。

### 非目标

- 不做增量复核（单迭代 / 单项目）——一次复核全工作区。
- 不做后台自动复核。
- 不在复核里做合并检查（那是 001）。

---

## 2. 已确认决策

1. **两档定义**（引用 `requirements.md §3.5`、`data-model.md §5`）：
   - 快扫 `list_workspaces(reconcile=false)`：枚举 `workspaceRoot` 一级子目录 → 读每个清单 → `archivedAt` 非空跳过 → 每条记录 `validity` 取 `removed`（`lifecycle = removed`）/ `missingDirectory`（目录不存在）/ `unknown`（目录存在）；`dirty`、`hasChanges`、`renamedFrom` 为 `null`；不扫 discovered。
   - 复核 `list_workspaces(reconcile=true)`：在快扫基础上，对每条 `active` 记录：源仓库 `worktree list --porcelain`（每仓库缓存一次）→ 路径是否注册（`notRegistered`）→ `head_state::judge`（`valid` / `headMismatch` / `Renamed`）→ `status --porcelain`（`dirty`）→ `hasChanges`（005）；源仓库不存在 → `sourceMissing`；再对迭代目录下每个含 `.git` 文件的子目录做 discovered 扫描（不在清单中 → `validity = discovered`，`lifecycle = null`）。
   - 被否决：三档（快扫 / 轻复核不跑 status / 全复核）。原因：多一档增加解释成本，`status` 开销可接受。
2. **复核持操作锁**（`acquire("复核状态")`），快扫不持锁。
   - 原因：011 可能原子回写清单 `branch`；若与创建/移除并发会出现两方同时写同一清单。
   - 被否决：复核只读、改名回写另开命令。原因：多一次往返且用户会漏点；规格明确复核含回写。
3. **复核中保留旧列表**，完成后用返回值**整体替换**（不逐迭代合并）。
   - 被否决：复核中清空列表显示骨架屏。原因：用户失去参照，且复核可能持续数秒。
4. **唯一触发点**：页面标题右侧「复核状态」按钮；卡片上不放单迭代复核。
   - 被否决：每个卡片一个复核按钮。原因：011 的同步计数与 discovered 扫描按全工作区语义更简单；规格「无刷新按钮」精神是避免用户养成频繁刷新的习惯。
5. **快扫触发时机**：启动、保存设置成功后、创建完成返回列表、移除成功、归档成功、备注/隐藏/恢复成功、排序/移动成功。这些操作完成后的列表状态都是 `unknown`（未复核），由用户决定何时复核。
   - 被否决：这些操作后自动复核。原因：违反「复核只由按钮触发」；且创建后立刻复核意义不大（刚创建必然有效）。
6. **无刷新按钮、无定时轮询、无窗口聚焦自动刷新**。
7. **复核结果 toast**：`list_workspaces(true)` 返回后，若 011 同步了 N（N > 0）个分支改名，toast success「已同步 N 个分支重命名」；N = 0 不提示。同步计数通过 `WorkspaceGroup.projects[].renamedFrom` 非空计数得出，**不新增返回字段**。
   - 被否决：返回值加 `renamedCount`。原因：可由投影推导，避免接口膨胀。
8. **复核失败**（`busy`、`workspaceRoot` 缺失等）→ toast danger，旧列表原样保留。
9. **快扫与复核对 `archivedAt`、`hiddenAt` 的处理一致**（`requirements.md §3.10`）。
10. **复核不清除内存中的合并检查结果**（`mergeResults`），行级 `hasChanges`/`dirty` 优先取合并检查值（006）。

---

## 3. 技术方案概要

### 3.1 数据结构（`data-model.md §5`）

`WorkspaceGroup`、`WorkspaceProject`。本功能使用的字段：`validity`、`dirty`、`hasChanges`、`renamedFrom`、`manifestHealth`、`manifestMessage`、`openable`、`removable`。无新增字段。

### 3.2 后端

模块：`manifest.rs`（列举与投影）、`head_state.rs`（011）。

```rust
/// 快扫或复核入口
pub fn list_groups(config: &AppConfig, reconcile: bool) -> Result<Vec<WorkspaceGroup>, AppError>;

/// 单迭代快扫投影（纯函数，可单测）
fn project_snapshot(manifest: &Manifest, iteration_dir: &Path) -> WorkspaceGroup;

/// 单迭代复核：在快扫结果上补 Git 事实；返回 (group, renamed_count)
fn reconcile_group(config: &AppConfig, snapshot: WorkspaceGroup, manifest: &mut Manifest,
                   repo_cache: &mut HashMap<PathBuf, Vec<WorktreeEntry>>) -> Result<WorkspaceGroup, AppError>;

/// discovered 扫描：迭代目录下含 `.git` 文件、路径不在清单中的子目录
fn scan_discovered(iteration_dir: &Path, manifest: &Manifest) -> Vec<WorkspaceProject>;
```

`lib.rs`：

```rust
#[tauri::command]
async fn list_workspaces(state: State<'_, OperationState>, reconcile: bool)
    -> Result<Vec<WorkspaceGroup>, AppErrorObject> {
    let _guard = if reconcile { Some(state.acquire("复核状态")?) } else { None };
    let config = config::load()?;
    if config.workspace_root.is_none() { return Ok(vec![]); }
    spawn_blocking(move || manifest::list_groups(&config, reconcile)).await ...
}
```

复核流程（每迭代）：

```text
1. 快扫投影 snapshot
2. manifestHealth ≠ valid → 直接返回 snapshot（不跑 Git）
3. 对每条 active 记录：
   a. 源仓库不存在或非 Git 根 → sourceMissing
   b. repo_cache 取/跑 worktree list --porcelain
   c. 路径（规范化、大小写不敏感）不在列表 → notRegistered
   d. 目录不存在 → missingDirectory
   e. head_state::judge → Valid / HeadMismatch / Renamed{live}
      Renamed → validity=valid、branchDisplay=live、renamedFrom=旧名、manifest.branch=live、标记 dirty_manifest
   f. status --porcelain 非空 → dirty=true
   g. hasChanges（005）
4. dirty_manifest → atomic_json::write_json_atomic(manifest)
5. scan_discovered 追加到 projects 末尾（validity=discovered、removable=true、openable=true）
```

### 3.3 Git 调用序列（`architecture.md §5` 原文，按顺序）

| 序 | 参数向量 | 工作目录 |
| --- | --- | --- |
| 1 | `rev-parse --show-toplevel` | 源仓库路径（确认是 Git 根） |
| 2 | `worktree list --porcelain` | 源仓库根（每仓库一次） |
| 3 | `show-ref --verify --quiet refs/heads/<branch>` | 源仓库根（011） |
| 4 | `rev-parse --verify HEAD` | worktree 目录 |
| 5 | `symbolic-ref -q --short HEAD` | worktree 目录 |
| 6 | `status --porcelain=v1 --untracked-files=all` | worktree 目录 |
| 7 | `rev-list --count <base>..HEAD` | worktree 目录（005） |

快扫不执行任何 Git 子命令。

### 3.4 前端

- `App.vue`：`loadWorkspaces(reconcile: boolean)`；启动时 `loadWorkspaces(false)`；各写操作成功后调用 `loadWorkspaces(false)`。
- `WorkspaceList.vue`：标题右侧「复核状态」按钮 → `reconciling = ref(false)`；点击 → `reconciling = true` → `await listWorkspaces(true)` → 成功：`groups.value = result`，统计 `renamedFrom` 非空数 N，N > 0 时 toast；失败：toast danger；`finally reconciling = false`。按钮 `:disabled="reconciling"` 并显示「复核中…」。
- 行级 `dirty` 列：`hasChanges === null` 显示「未复核」neutral（`ui-spec.md §4.1`）。
- 无任何刷新按钮、`setInterval`、`visibilitychange` 监听。

### 3.5 事件

无。

---

## 4. 状态与枚举

| 枚举 | 值 | 快扫可出现 | 复核可出现 |
| --- | --- | --- | --- |
| `Validity` | `unknown` | 是 | 否 |
| | `valid` | 否 | 是 |
| | `missingDirectory` | 是 | 是 |
| | `notRegistered` | 否 | 是 |
| | `headMismatch` | 否 | 是 |
| | `sourceMissing` | 否 | 是 |
| | `removed` | 是 | 是 |
| | `discovered` | 否 | 是 |
| `ManifestHealth` | `valid \| missing \| damaged` | 是 | 是 |

`dirty` / `hasChanges`：快扫 `null`，复核 `boolean`（`hasChanges` 解析链全部失败时仍为 `null`）。

---

## 5. 测试要点（对应 `implementation-plan.md` S2 `manifest.rs` 与 S3）

1. 快扫投影单测：`manifestHealth × validity` 组合（S2 已列）：valid + 目录存在 → `unknown`；valid + 缺目录 → `missingDirectory`；`removed` → `removed`；`missing`/`damaged` → `projects` 为空且 `manifestMessage` 非空。
2. 快扫不调用 Git：用 `GitRunner` 的计数 mock 或在 `PATH` 中放置假 `git`（退出码 1）断言快扫仍成功。
3. 复核集成：创建后 `reconcile=true` → `valid`；手工 `rm -rf` 目录 → `missingDirectory`；`git worktree remove` 后目录重建 → `notRegistered`；在 worktree 里 `git checkout -b other`（清单分支仍在）→ `headMismatch` 且清单不变；改名 fixture → `valid` + `renamedFrom`（011）。
4. discovered：在迭代目录手工 `git worktree add` 一个未托管目录 → 复核出现 `discovered` 行；快扫不出现。
5. 操作锁：持锁期间 `list_workspaces(true)` 得 `busy`；`list_workspaces(false)` 正常返回。
6. `archivedAt` 非空迭代在两档都不出现。
7. `workspaceRoot = null` → 两档都返回 `[]`。
8. 前端 SSR：`WorkspaceList` 渲染含「复核状态」按钮文案；不含「刷新」。
9. 手工：复核中列表不闪空；完成后 `unknown` 变 `valid`；N > 0 时看到 toast。

---

## 6. 已知局限

- 复核是全工作区级别，迭代很多时耗时线性增长，无法只复核一个迭代。
- 快扫的 `unknown` 状态无法区分「有效」与「已损坏但目录还在」，必须复核才知道。
- 复核期间磁盘变化（用户另开终端操作）不会被感知，需再次点击。
- discovered 扫描只看迭代目录一级子目录是否含 `.git` 文件，嵌套更深的 worktree 不会被发现。
- 无自动刷新意味着外部工具改动后列表可能长期陈旧，这是刻意取舍。

---

## 7. 实现记录

- 完成日期：2026-09-30（S3 落地，S5 补 hasChanges）
- 偏离项：无。复核持锁「复核状态」、快扫不持锁、按钮 loading + 旧列表保留、无刷新按钮与轮询（`grep setInterval` 无命中）。
  实现说明：复核的 discovered 扫描优先按已配置仓库的 `worktree list` 条目匹配，迭代目录下含 `.git` 但未匹配到仓库的子目录再补一条 `sourceRepository` 为空的行。
- 原因：保证「未托管但有来源」的行可移除，「来源未知」的行不可移除。
- 涉及文件：`src-tauri/src/manifest.rs`、`src-tauri/src/lib.rs`、`src/components/WorkspaceList.vue`

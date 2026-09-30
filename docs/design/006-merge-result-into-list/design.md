# 006 · 合并检查结果并入「基准变动」列

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 关联：`requirements.md §3.6`、`data-model.md §4/§5`、`ui-spec.md §4.1`、`design/003`、`design/005`、`implementation-plan.md S5`。

---

## 1. 背景与目标

### 现状

「基准变动」列的 `hasChanges`/`dirty` 在复核档（`list_workspaces(reconcile=true)`）才会有值，快扫档恒为 `null`（「未复核」）。合并检查（001/003）本来就要在每个 worktree 里跑 `fetch` 与若干 Git 命令，并且它拿到的远端跟踪引用比复核档更新（见 `design/005 §6` 的局限）。如果用户点了「检查合并」，列表却仍显示「未复核」，既浪费了刚算过的信息，也让「基准变动」与合并矩阵两列的结论可能互相矛盾。

### 目标

- `MergeRecordResult` 携带 `hasChanges`/`dirty`，由 005 的同一函数在合并检查过程中计算。
- 前端收到 `merge-check-progress`（`phase = record`）时，按 `worktreePath` 就地更新对应行的 `hasChanges`、`dirty`、`develop`、`master` 四个格，**不另跑复核**。
- 定义合并结论的生命周期：什么时候生效、什么时候作废。

### 非目标

- 不把合并检查结果落盘（仍是内存快照，见 003）。
- 不改变复核档的计算方式（005 已定义）。
- 不在合并检查里更新 `validity`（那是复核的职责；合并检查不做 `worktree list` 校验）。

---

## 2. 已确认决策

1. **`MergeRecordResult` 新增 `hasChanges: boolean | null`、`dirty: boolean | null`，由后端在逐条检查时用 `head_state::has_changes` / `is_dirty` 计算。**
   被否决备选：合并检查完成后前端自动触发一次复核。原因：复核持操作锁、跑 `worktree list` + 全量 `status`，代价高且与 004「复核只由按钮触发」冲突。
2. **列表行取值优先级：合并检查结果 > 复核结果 > `null`。**
   即前端渲染 `dirty` 列时先查该迭代的 `MergeCheckResult` 中是否有同 `worktreePath` 的记录，有则用它；否则用 `WorkspaceProject` 自身的值。
   被否决备选：合并检查结果直接写回 `groups` 中的 `WorkspaceProject`。原因：`groups` 是「清单 + 复核」的投影，被合并结果污染后重新快扫会丢失、再复核又会覆盖，生命周期混乱；用独立 `Map` 叠加渲染，两者各自有明确的作废时机。
3. **就地更新只改四个格：`hasChanges`、`dirty`、`mergeDevelop`、`mergeMaster`。** `validity`、`branchDisplay`、`renamedFrom` 等不动。
   被否决备选：用 `record.branchDisplay` 顺带更新分支列。原因：分支列的权威来源是复核（含 011 改名回写），合并检查只是「按 live 名判定」但不负责回写与 `renamedFrom` 标记，两边混用会让 011 的 toast 计数与列表不一致。
4. **合并结论按 `iteration` 作废，时机固定为：**
   - 该迭代发生创建（`create_workspaces` 成功返回后，对 `request.iteration`）；
   - 该迭代发生移除（`remove_worktree` / `remove_discovered_worktree` 成功后）；
   - 该迭代发生归档（`archive_iteration` 返回后，无论 `archived` 与否，因为可能已有记录被移除）；
   - 014 跨迭代移动成功后，源与目标两个迭代都作废；
   - 014 同迭代排序**不作废**（记录内容未变，只换顺序；就地更新按 `worktreePath` 匹配，与顺序无关）；
   - 用户再次点击「检查合并」时，该迭代旧结果被新结果整体替换（新结果开始前先清空，避免旧行与新行混显）。
   被否决备选：复核（`listWorkspaces(true)`）后也清空。原因：复核与合并检查回答的是不同问题（有效性 vs 合并状态），复核不会让合并结论过时；保留可减少一次不必要的联网检查。
5. **前端匹配键是 `worktreePath` 的字符串严格相等**，后端保证两条路径都来自清单的规范化值。
   被否决备选：`projectId`。原因：同一 `projectId` 允许多条记录（`p`、`p-2`）。
6. **`hasChanges`/`dirty` 在 `record` 中允许为 `null`**（目录缺失、解析链失败），此时前端按 `null` 渲染「未复核」，**不**回退到复核值。
   被否决备选：`null` 时回退到复核值。原因：合并检查是更新的观测，若它判定不可得而复核给了旧值，展示旧值会误导；保持「新观测覆盖旧观测」的单一规则。

---

## 3. 技术方案概要

### 数据结构（引用 `data-model.md`）

- `MergeRecordResult { projectId, branchDisplay, worktreePath, lifecycle, develop, master, hasChanges, dirty }`：本功能使用 `hasChanges`、`dirty`（已在 `data-model.md §4` 定义）。
- `MergeCheckProgress.record: MergeRecordResult | null`。
- `WorkspaceProject.hasChanges/dirty`：复核档来源，作为回退值。

不新增持久化字段。

### 后端

模块：`src-tauri/src/merge_check.rs`，逐条检查时调用 `head_state.rs`（005）。

```rust
/// 单条记录的合并检查；在 develop/master 判定完成后追加 hasChanges/dirty。
fn check_record(
    git: &GitRunner,            // repo = worktree 目录
    remotes: &[String],
    record: &ManifestProject,
    live_branch: Option<&str>,  // 由 head_state 判定后的 live 名（011）
    stale: bool,
) -> MergeRecordResult {
    // ... develop / master 三层判定（001）
    let base = head_state::resolve_base(git, Some(&record.base_ref))?;
    let has_changes = head_state::has_changes(git, base.as_ref())?;   // Option<bool>
    let dirty = head_state::is_dirty(git)?;                            // bool → Some(dirty)
    // MergeCellResult.dirty 与顶层 dirty 取同一值
}
```

目录缺失（`notCheckable`）时 `hasChanges = None`、`dirty = None`，不调用 Git。

#### Git 调用序列（参数向量按 `architecture.md §5`，按执行顺序）

在 001 每条记录的判定之后追加：

1. `rev-parse --verify <candidate>^{commit}`（解析链，至多 4 次）
2. `rev-list --count <base>..HEAD`
3. `status --porcelain=v1 --untracked-files=all`

`remote` 结果与 001 共用同批缓存，不重复调用。

### 前端

- 入口组件：`WorkspaceList.vue`（持有 `mergeResults: Map<string, MergeCheckResult>`）、`IterationCard.vue`（接收该迭代的 `MergeCheckResult | undefined` 作为 prop 并按行合并渲染）。
- `api/tauri.ts`：`checkMergeStatus(iteration, projectId?, worktreePath?)`、`onMergeCheckProgress(handler)`（返回 unlisten）。
- 状态存放：仅 `WorkspaceList.vue` 内 `ref`，不进 localStorage，不进 `App.vue` 的 `groups`。
- 渲染取值（`utils/merge.ts` 提供纯函数，供 SSR 测试）：

```ts
export function effectiveChangeState(
  project: WorkspaceProject,
  record: MergeRecordResult | undefined,
): { hasChanges: boolean | null; dirty: boolean | null } {
  return record
    ? { hasChanges: record.hasChanges, dirty: record.dirty }
    : { hasChanges: project.hasChanges, dirty: project.dirty };
}
```

- 就地更新：`onMergeCheckProgress` 收到 `phase === "record"` 时，`mergeResults.get(iteration).records` 中按 `worktreePath` 替换或追加该条；Vue 响应式使对应行重渲染。
- 作废：`WorkspaceList.vue` 暴露 `invalidateMergeResult(iteration: string)`；`App.vue` 在创建/移除/归档/移动成功回调里调用（014 移动时调用两次）。点击「检查合并」时先 `mergeResults.delete(iteration)` 再发命令。

### 事件

复用 `merge-check-progress`（003），无新事件。`phase = record` 的 `record` 字段是本功能的唯一数据通道。

---

## 4. 状态与枚举

| 字段 | 取值 | 含义 |
| --- | --- | --- |
| `MergeRecordResult.hasChanges` | `true / false / null` | 与 005 相同定义；`null` = 目录缺失或解析链失败 |
| `MergeRecordResult.dirty` | `true / false / null` | `status --porcelain` 非空；`null` = 目录缺失 |
| `MergeCellResult.dirty` | `true / false` | 与顶层 `dirty` 同值（`null` 时为 `false`），供 008 归档 clean 判定单元格级使用 |
| `MergeCheckPhase` | `fetching \| checking \| record` | 只有 `record` 携带非空 `record` |

「基准变动」列文案映射见 `ui-spec.md §8`（有变更 / 无 / 未复核 / 未提交）。

---

## 5. 测试要点

后端（`tempfile` + 裸仓库，`implementation-plan.md S5`）：

1. 合并检查返回的 `record.hasChanges/dirty` 与同 fixture 下复核档 `WorkspaceProject.hasChanges/dirty` 相等（与 005 测试 13 共用）。
2. 目录缺失记录：`develop.status = notCheckable`，`hasChanges = null`、`dirty = null`。
3. `MergeCellResult.dirty` 与顶层 `dirty` 一致。
4. 事件序列：对 N 条 active 记录，`phase = record` 事件恰好 N 次，且每次 `record.worktreePath` 唯一。

前端（`scripts/test-frontend.mjs`，SSR + `assert/strict`）：

5. `effectiveChangeState`：有 record 时取 record 值（含 record 为 `null` 值时不回退）；无 record 时取 project 值。
6. `IterationCard` 渲染：传入 project `hasChanges = null` 与 record `hasChanges = true` → 输出含「有变更」；传入 record `dirty = true` → 输出含「未提交」。
7. `mergeResults` 更新逻辑（抽成纯函数 `applyRecord(result, record)`）：同 `worktreePath` 替换、不同则追加、顺序无关。
8. 作废逻辑（纯函数）：`invalidate(map, iteration)` 只删指定迭代。

真实窗口手工验证：点「检查合并」，观察某行「基准变动」从「未复核」变为「有变更/无」而 `validity` 列不变；随后移除一行，该迭代合并格回到未检查态。

---

## 6. 已知局限

- **合并结论是内存快照**：切换页面、重启应用后消失；重新进入列表需再点「检查合并」。
- **覆盖是单向的**：合并检查后再点「复核状态」，列表仍显示合并检查的 `hasChanges/dirty`（决策 4），即使复核是更晚的观测。两者都基于本地 Git 状态，实际差异极小；若用户在两次操作之间改了工作区，需重新检查合并才能刷新。
- **`validity` 不随合并检查更新**：合并检查期间 worktree 被外部删除，合并格显示 `notCheckable`，但状态列仍是复核时的值。
- **同迭代排序后不作废**是按 `worktreePath` 匹配的前提下才成立；若将来记录标识改变，需重新评估。
- **未提交改动只影响 `dirty`**：合并矩阵对比的是分支 tip，工作区改动不在任何分支中。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

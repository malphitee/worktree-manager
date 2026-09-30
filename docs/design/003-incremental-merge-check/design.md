# 003 · 增量合并检查（事件流）

> 设计文档是**当次决策的历史记录**，设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 相关基线：`docs/architecture.md §7.1 §7.2 §7.3`、`docs/data-model.md §4`、`docs/design/001-merge-check/design.md`。

---

## 1. 背景与目标

### 现状

001 的合并检查对 10+ 个仓库各跑一次 fetch 与多次 Git 子命令，耗时以十秒计。若命令只在最后一次性返回，用户在此期间看不到任何变化，也无法判断是卡住还是在跑。

### 目标

- 一次 `check_merge_status` 调用 + `merge-check-progress` 事件流；每条记录判定完成即推送，前端逐行就地更新。
- 命令最终返回值仍是完整 `MergeCheckResult`，事件丢失时可兜底整体替换。
- 支持单项目过滤（重查一条），复用同一命令与事件。

### 非目标

- 不实现取消。
- 不做并行判定（仓库间串行，简单且日志可读）。
- 不做跨会话缓存。

---

## 2. 已确认决策

1. **一次命令 + 事件流**，不拆成「开始检查」「轮询结果」两个命令。
   - 被否决：前端逐项目调用 N 次命令。原因：每次都要重新获取操作锁与 fetch，且无法保证同仓库只 fetch 一次。
   - 被否决：后端起任务、前端轮询状态命令。原因：引入任务表与生命周期管理，规格明确不做取消/后台任务。
2. **事件载荷 `MergeCheckProgress`** 固定为 `{ iteration, projectId, worktreePath, index, total, phase, message, record }`；`phase = record` 时 `record` 非空，其余为 `null`。
3. **phase 顺序**：每个仓库先 `fetching`（一次），随后该仓库每条记录 `checking` → `record`。`index` 从 0 计，`total` 为本次待查记录总数（过滤后）。
4. **命令返回值是完整结果**：前端收到返回值后用它整体替换该迭代的 `MergeCheckResult`（覆盖事件累积的中间态），保证事件丢失或乱序也能收敛。
   - 被否决：命令只返回 `()`。原因：Tauri 事件不保证送达，无兜底会出现「永远转圈」。
5. **前端状态**：`WorkspaceList.vue` 内 `mergeResults: Map<iteration, MergeCheckResult>`；`record` 事件按 `worktreePath` 找行就地替换（找不到则追加）；`checkedAt` 在收到返回值时更新。
6. **过滤参数语义**：`projectId` 与 `worktreePath` 都可选；给 `worktreePath` 精确匹配一条（规范化后大小写不敏感比较）；只给 `projectId` 匹配该 id 的全部 active 记录；都不给则全迭代。过滤结果为空 → 返回 `records = []`，不报错。单条重查时前端只替换该 `worktreePath` 的记录，不清其他行。
7. **`message` 写实际执行的命令**（如 `git fetch --no-tags origin develop master`、`git merge-base --is-ancestor feature/x origin/develop`），便于用户理解卡在哪。
8. **组件卸载必须 `unlisten()`**；订阅在 `WorkspaceList.vue` `onMounted` 建立一次，`onBeforeUnmount` 释放，而不是每次点击订阅。
9. **进度期间 UI**：该迭代「检查合并」按钮 loading；已完成的行立即显示结果，未完成的行显示「检查中…」neutral 标签；`fetching` 阶段在卡片头显示一行 `message`。
10. **错误**：命令 reject（如 `busy`、清单损坏）→ toast danger，该迭代按钮恢复，已收到的部分结果保留但 `checkedAt` 不更新。

---

## 3. 技术方案概要

### 3.1 数据结构（`data-model.md §4`）

- `MergeCheckProgress { iteration, projectId, worktreePath, index: number, total: number, phase: MergeCheckPhase, message: string, record: MergeRecordResult | null }`
- `MergeCheckResult`、`MergeRecordResult`（001）。

### 3.2 后端

`lib.rs`：

```rust
#[tauri::command]
async fn check_merge_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, OperationState>,
    iteration: String,
    project_id: Option<String>,
    worktree_path: Option<String>,
) -> Result<MergeCheckResult, AppErrorObject> {
    let _guard = state.acquire("检查合并")?;
    let config = config::load()?;
    tauri::async_runtime::spawn_blocking(move || {
        merge_check::check(&config, &iteration, MergeFilter { project_id, worktree_path },
            &mut |p| { let _ = app.emit("merge-check-progress", &p); })
    }).await.map_err(...)?.map_err(Into::into)
}
```

`merge_check::check` 内的 emit 时序（引用 001 §3.2 流程）：

```text
for repo in groups:
    emit { phase: fetching, index: first_index_of_repo, projectId/worktreePath: 该仓库第一条, message: "git fetch --no-tags origin develop master", record: null }
    for rec in repo.records:
        emit { phase: checking, index, message: "git merge-base --is-ancestor <subject> origin/develop", record: null }
        …判定…
        emit { phase: record, index, message: "完成", record: Some(result) }
```

emit 失败（无窗口）忽略，不影响返回值。

### 3.3 Git 调用序列

同 001 §3.3；本功能不新增 Git 调用。

### 3.4 前端

`api/tauri.ts`：

```ts
export function checkMergeStatus(iteration: string, filter?: { projectId?: string; worktreePath?: string }): Promise<MergeCheckResult>
export function onMergeCheckProgress(handler: (p: MergeCheckProgress) => void): Promise<() => void>
```

浏览器演示模式：`checkMergeStatus` reject `unsupported`；`onMergeCheckProgress` 返回空 unlisten。`@tauri-apps/api/event` 在函数内动态 import。

`WorkspaceList.vue`：

```ts
const mergeResults = ref(new Map<string, MergeCheckResult>())
const checking = ref(new Set<string>())        // 正在检查的 iteration
const progressLine = ref(new Map<string, string>()) // iteration → 当前 message
let unlisten: (() => void) | null = null

onMounted(async () => { unlisten = await onMergeCheckProgress(applyProgress) })
onBeforeUnmount(() => { unlisten?.(); unlisten = null })

function applyProgress(p: MergeCheckProgress) {
  progressLine.value.set(p.iteration, p.message)
  if (p.phase !== "record" || !p.record) return
  const cur = mergeResults.value.get(p.iteration) ?? { iteration: p.iteration, checkedAt: "", records: [] }
  const i = cur.records.findIndex(r => r.worktreePath === p.record!.worktreePath)
  i >= 0 ? cur.records.splice(i, 1, p.record) : cur.records.push(p.record)
  mergeResults.value.set(p.iteration, { ...cur })   // 触发响应
}

async function runMergeCheck(iteration: string, filter?) {
  checking.value.add(iteration)
  try {
    const result = await checkMergeStatus(iteration, filter)
    if (filter?.worktreePath) { /* 只替换该行 + 更新 checkedAt */ } else { mergeResults.value.set(iteration, result) }
  } catch (e) { toast("danger", e.message) }
  finally { checking.value.delete(iteration); progressLine.value.delete(iteration) }
}
```

`IterationCard.vue` 通过 props 接收 `mergeResult`、`checking`、`progressLine`；行级按 `worktreePath` 查找结果。

### 3.5 事件

| 事件 | 触发 | 频率 |
| --- | --- | --- |
| `merge-check-progress` | 见 §3.2 | 每仓库 1 次 `fetching` + 每记录 1 次 `checking` + 1 次 `record` |

---

## 4. 状态与枚举

| 枚举 | 值 | 含义 |
| --- | --- | --- |
| `MergeCheckPhase` | `fetching` | 正在对某仓库 fetch 目标分支 |
| | `checking` | 正在判定某条记录 |
| | `record` | 某条记录判定完成，`record` 携带结果 |

前端派生状态：`checking: Set<iteration>`、`progressLine: Map<iteration, string>`。

---

## 5. 测试要点（对应 `implementation-plan.md` S5）

1. 后端：用收集型 `emit` 闭包断言事件序列：每仓库恰好 1 个 `fetching`；每记录 `checking` 后紧跟 `record`；`record.record` 非空且 `worktreePath` 匹配；`index` 单调、`total` 恒等。
2. 后端：`worktreePath` 过滤 → `total = 1`，只有 3 个事件（fetching/checking/record）。
3. 后端：过滤无匹配 → `records = []`、无事件、不报错。
4. 后端：命令返回值与 `record` 事件累积结果逐字段相等。
5. 前端 SSR：`applyProgress` 纯函数化后单测（抽到 `utils/merge.ts::applyRecord(map, progress)`）：新增、替换、非 record 阶段不改动。
6. 手工（真实窗口）：点「检查合并」后行逐条变化；关闭列表页再回来不重复订阅（控制台无重复日志）；检查过程中切换页面再回来，结果仍在（`mergeResults` 存在 `WorkspaceList` 生命周期内；若页面切换会卸载 `WorkspaceList`，则接受结果丢失并在 §7 记录）。

---

## 6. 已知局限

- 事件不保证送达；依赖命令返回值兜底，中间态可能短暂缺行。
- 仓库间串行，总耗时等于各仓库耗时之和；不做并行是刻意取舍。
- 页面切换若卸载 `WorkspaceList.vue`，内存结果随之丢失，需重新检查。
- 单条重查仍会对该仓库 fetch 一次。
- 无取消：用户关闭窗口前长 fetch 会继续跑到超时或完成。

---

## 7. 实现记录

- 完成日期：2026-09-30（S5）
- 偏离项：无。订阅一次并在 `onBeforeUnmount` 释放；`record` 事件按 `worktreePath` 就地更新（`utils/merge.ts::applyRecord`，纯函数可测）；命令返回值整体替换。
- 原因：—
- 涉及文件：`src/components/WorkspaceList.vue`、`src/utils/merge.ts`、`src-tauri/src/merge_check.rs`、`src-tauri/src/lib.rs`

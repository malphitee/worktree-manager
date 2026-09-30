# 008 · 迭代归档

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 关联：`requirements.md §3.9/§3.10`、`data-model.md §2/§4/§5`、`architecture.md §5/§6/§7`、`ui-spec.md §6/§7.1`、`design/001`、`design/007`、`implementation-plan.md S6`。

---

## 1. 背景与目标

### 现状

一个迭代结束后，用户要逐个 worktree 打开移除对话框、看风险、输入确认文本、点移除，10+ 个仓库要重复 10+ 次；做完之后迭代卡片还留在列表里（所有行都是「已移除」），只能靠 010 隐藏。此外单个移除（`removal.rs`）的风险模型是「任何本侧独有提交都阻止」，但迭代结束时的核心问题其实是「改动有没有合并进 develop / master」，两者标准不同。

### 目标

- 提供「归档」：一次评估整个迭代（复用 001 的合并判定），给出每条记录是否 `clean` 与阻塞原因；确认后批量移除全部 `active` worktree；全部成功后写 `archivedAt`，迭代从列表消失。
- 干净记录用不带 `--force` 的 `worktree remove`；只有用户明确勾选「强制归档」时，不干净的记录才带 `--force`。
- 归档**不是删除**：保留本地分支、迭代目录、公共目录、清单（含全部历史记录）。

### 非目标

- 不删除迭代目录、公共目录快照、本地分支、远端分支。
- 不提供「取消归档」命令（`archivedAt` 只能手工改清单）。
- 不处理 discovered worktree（它们不在清单里，原样保留）。
- 不做部分归档（只归档选中的几条）。
- 不做整迭代删除（见 `todo.md`）。

---

## 2. 已确认决策

1. **评估与执行分成两个命令：`assess_archive`（持操作锁「评估归档」，因为它会 fetch，需与检查合并互斥）与 `archive_iteration`（持操作锁「归档迭代」）。**
   被否决备选：一个命令内先评估再直接执行。原因：用户必须先看到 blockers 再决定是否强制；且评估要 `fetch`，耗时明显，需要通过事件流展示进度。
2. **`assess_archive` 复用 `merge_check::check`，事件通过 `archive-progress` 推送，`record` 恒为 `null`。**
   被否决备选：复用 `merge-check-progress` 事件名。原因：列表页可能同时订阅了 `merge-check-progress` 做就地更新（006），归档评估的中间结果不应污染列表的合并结论；用独立事件名让 `ArchiveDialog` 单独订阅。`record = null` 是因为对话框只需进度文案，最终结果由命令返回值整体给出。
3. **`clean` 四条件全部由后端判定，前端只显示**：`develop.status ∈ {merged, contained, targetMissing}`、`master.status ∈ {merged, contained, targetMissing}`、`dirty == false`、`stale == false`（该仓库本次 fetch 成功）。`blockers` 文案也由后端生成。
   被否决备选：前端根据 `MergeCellStatus` 自行推导。原因：`architecture.md §4.3` 信任模型——安全结论一律后端产生；前端推导会在 008 与 001 的展示规则之间产生第二份逻辑。
   被否决备选：`targetMissing` 视为不干净。原因：仓库本来就没有 `develop`（很多只用 `master` 的仓库），把它算成阻塞会让这类仓库永远无法「干净归档」。
   被否决备选：`stale` 只是警告不算阻塞。原因：fetch 失败时 `merged` 结论基于旧的远端跟踪引用，可能已被远端 force-push 覆盖；归档会移除 worktree，这个方向的误判代价大。
4. **`confirmationText` 就是迭代号；`archive_iteration` 要求 `request.confirmation` 与迭代号严格相等（字节级，不 trim）。**
   被否决备选：`{迭代号}/archive` 之类的复合文本。原因：移除单个 worktree 用 `{迭代号}/{目录名}` 是为了区分同迭代下多个目录；归档粒度就是迭代，迭代号本身已足够明确。
5. **`archive_iteration` 执行前重新评估（不信任前端传来的 clean）；`!clean && !force` 整体拒绝，不动任何记录。**
   被否决备选：只用评估结果的 `checkedAt` 做过期校验。原因：评估与执行之间用户可能改了工作区，重新评估的成本（一次 fetch + 判定）相对于误删可接受。
6. **逐条 `worktree remove`：干净记录不带 `--force`；`force = true` 且该记录不干净时带**单个** `--force`；单条失败记录到 `failed[]` 并继续下一条；每条成功后立即原子写清单（`lifecycle = removed`、`removedAt`）。**
   被否决备选：一条失败即中止。原因：与判定口径 1「失败隔离」一致；已成功移除的不可能回滚，中止只会留下更混乱的中间态。
   被否决备选：`--force --force`（Git 对 locked worktree 的双 force）。原因：`worktreeLocked` 是明确的人为信号，不允许绕过；locked 记录在 `force` 模式下同样进 `failed[]`。
7. **全部成功（`failed.is_empty()`）才写 `archivedAt`；否则 `archived = false`，已移除的记录保持 `removed`，用户修正后可再次归档。**
   被否决备选：部分成功也写 `archivedAt`。原因：`archivedAt` 会让迭代从列表消失，剩下的 active worktree 就没有任何入口可见。
8. **`list_groups` 在快扫与复核两档都直接跳过 `archivedAt` 非空的迭代**，优先级高于 `hiddenAt`。
   被否决备选：归档迭代进「已隐藏」收纳区。原因：收纳区是可恢复的临时收纳；归档是终态，混在一起会诱导用户「恢复」一个已经没有 worktree 的迭代。
9. **归档不清 `hiddenAt`、不改 `note`**，清单其他字段原样保留作历史。
10. **不干净时对话框显示每条记录的 blockers 列表 + 「强制归档（对不干净记录使用 --force，将丢弃其未提交改动）」复选框**；干净时只显示确认输入框。
    被否决备选：不干净时不允许归档。原因：用户可能有意放弃某些改动；提供带明确警告的 force 路径比迫使用户去终端跑 `git worktree remove --force` 更安全（至少走了清单更新）。

---

## 3. 技术方案概要

### 数据结构（引用 `data-model.md`）

使用：

- 清单 `archivedAt: string | null`（本功能唯一写入的新字段）、`projects[].lifecycle`、`projects[].removedAt`。
- `ArchiveRequest { iteration, confirmation, force }`。
- `ArchiveAssessment { iteration, confirmationText, checkedAt, clean, records[]: { projectId, branchDisplay, worktreePath, develop: MergeCellResult, master: MergeCellResult, dirty, clean, blockers: string[] } }`。
- `ArchiveOutcome { iteration, removedCount, failed[]: { projectId, worktreePath, message }, archived }`。
- `MergeCheckProgress`（作为 `archive-progress` 载荷，`record = null`）。
- `WorkspaceGroup`：不含 `archivedAt`（归档迭代根本不进投影）。

### 后端

模块：`src-tauri/src/archive.rs`，依赖 `merge_check.rs`（001/007）、`removal.rs` 的执行原语、`manifest.rs`、`atomic_json.rs`。

```rust
/// 评估：读清单 → merge_check::check（emit 转发为 archive-progress）→ 逐条 clean/blockers。
pub fn assess(
    config: &AppConfig,
    iteration: &str,
    emit: &dyn Fn(MergeCheckProgress),
) -> Result<ArchiveAssessment, AppError>;

/// 单条 clean 判定与 blockers 文案（纯函数，便于单测）。
pub fn judge_record(r: &MergeRecordResult) -> (bool, Vec<String>);

/// 执行：校验 confirmation → 重新 assess → (!clean && !force) 拒绝 →
/// 逐条 worktree remove [--force] → 每条后写清单 → 全成功写 archivedAt。
pub fn archive(
    config: &AppConfig,
    request: &ArchiveRequest,
    emit: &dyn Fn(MergeCheckProgress),
) -> Result<ArchiveOutcome, AppError>;
```

`judge_record` 的 blockers 文案（中文，固定措辞，前端直接显示）：

| 条件 | 文案 |
| --- | --- |
| `develop.status ∉ {merged, contained, targetMissing}` | `develop：{状态文案}`（如 `develop：未合并（3 个提交）`、`develop：分支不存在`、`develop：检查出错：{errorMessage}`） |
| `master.status ∉ {...}` | `master：{状态文案}` |
| `dirty == true` | `存在未提交改动` |
| `stale == true`（任一格） | `本次 fetch 失败，合并结论可能过时` |

`clean = blockers.is_empty()`；`ArchiveAssessment.clean = records.iter().all(|r| r.clean)`（无 active 记录时为 `true`，见 007 决策 5）。

前置校验（`assess` 与 `archive` 共用）：迭代号合法；`workspaceRoot` 非空；清单 `manifestHealth == valid`（`missing`/`damaged` → `ManifestDamaged` 错误「清单缺失或损坏，无法归档」）；`archivedAt` 已非空 → `Conflict("该迭代已归档")`。

`archive` 的 `worktree remove` 通过 `removal.rs` 暴露的底层原语执行（`GitRunner{repo: 源仓库}.worktree_remove(path, force)`），**不**走单个移除的风险评估（那套风险比归档 clean 更严格，且已由 clean/force 决定）。目录已不存在但仍注册（`prunable`）的记录：不带 `--force` 会失败进 `failed[]`；`force` 模式下同样只用单个 `--force`，失败照样进 `failed[]`。

#### Git 调用序列（参数向量按 `architecture.md §5`，按执行顺序）

`assess`（每个源仓库去重一次 1；每条 active 记录 2–7）：

1. `fetch --no-tags origin develop master`
2. `worktree list --porcelain`
3. `show-ref --verify --quiet refs/heads/<branch>`、`symbolic-ref -q --short HEAD`、`rev-parse --verify HEAD`
4. `merge-base --is-ancestor <branch> origin/<target>`（develop、master 各一次）
5. `cherry origin/<target> <branch>`（层 1 未命中才执行）
6. `merge-tree --write-tree origin/<target> <branch>` + `rev-parse origin/<target>^{tree}`（层 2 未命中才执行）
7. `rev-parse --verify <candidate>^{commit}`、`rev-list --count <base>..HEAD`、`status --porcelain=v1 --untracked-files=all`

`archive`：先完整重跑上述 `assess` 序列，然后对每条 active 记录：

8. `worktree remove <path>`（干净）或 `worktree remove --force <path>`（`force && !clean`）

不执行 `branch -d`、`stash`、`commit`、`push`、`worktree prune`。

写清单：每条成功后 `atomic_json::write_json_atomic` 一次；全部成功后再写一次（`archivedAt = now`）。

### 前端

- 入口组件：`IterationCard.vue` 头部按钮组「归档」→ 打开 `ArchiveDialog.vue`（`<Teleport to="body">`，`Esc` 不关闭）。
- `api/tauri.ts`：`assessArchive(iteration)`、`archiveIteration(request)`、`onArchiveProgress(handler)`。
- 对话框状态机：`assessing`（显示进度文案，来自 `archive-progress.message`）→ `ready`（显示评估表：项目 / 分支 / develop / master / 未提交 / 结论；不干净行展开 blockers）→ `archiving`（按钮 loading，再次订阅进度）→ 关闭。
- 确认输入框 placeholder 为迭代号；「归档」按钮在 `confirmation !== iteration` 时禁用；不干净且未勾选「强制归档」时禁用并提示「存在 N 条不干净记录，请先处理或勾选强制归档」。
- 结果处理：`archived = true` → toast success「已归档迭代 {iteration}，移除 {removedCount} 个 worktree」，调用 006 的 `invalidateMergeResult(iteration)`，`listWorkspaces(false)`；`archived = false` → toast warning「{failed.length} 个 worktree 移除失败，迭代未归档」，对话框内列出 `failed[]`，同样作废合并结论并快扫。
- `manifestHealth ≠ valid` 的卡片「归档」按钮禁用（`ui-spec.md §7.1`）。
- 组件卸载/对话框关闭时 `unlisten()`。

### 事件

`archive-progress`：载荷 `MergeCheckProgress`，`record` 恒 `null`；`phase ∈ {fetching, checking, record}` 与 001 相同（`record` 阶段仅表示「该条完成」）。`archive` 执行阶段的移除进度复用同一事件，`message` 为实际命令（如 `git worktree remove --force /path`）。

---

## 4. 状态与枚举

| 字段 | 取值 | 含义 |
| --- | --- | --- |
| 清单 `archivedAt` | `null` | 未归档，正常投影 |
| | RFC 3339 | 已归档；`list_groups` 跳过；优先级高于 `hiddenAt` |
| `ArchiveAssessment.records[].clean` | `true` | 四条件全满足，移除时不带 `--force` |
| | `false` | `blockers` 非空 |
| `ArchiveAssessment.clean` | `true / false` | 全部记录 clean（无记录时 `true`） |
| `ArchiveOutcome.archived` | `true` | `failed` 为空且 `archivedAt` 已写 |
| | `false` | 至少一条失败；已成功的记录已是 `removed` |
| `Lifecycle` | `active → removed` | 每条成功移除后写入，附 `removedAt` |
| `MergeCellStatus` 视为干净 | `merged`、`contained`、`targetMissing` | |
| `MergeCellStatus` 视为阻塞 | `unmerged`、`branchMissing`、`notCheckable`、`error` | |
| `MergeCellResult.stale` | `true` | 阻塞 |
| `MergeCellResult.dirty` / 顶层 `dirty` | `true` | 阻塞 |
| `ErrorCode` | `validation` | confirmation 不匹配；`!clean && !force` |
| | `manifestDamaged` | 清单缺失/损坏 |
| | `conflict` | 已归档 |
| | `busy` | 操作锁被占 |

---

## 5. 测试要点

（`tempfile` + 裸仓库，`implementation-plan.md S6`。）

`judge_record` 纯函数：

1. 四种组合：`merged/merged/!dirty/!stale` → clean；`contained/targetMissing/!dirty/!stale` → clean；`unmerged/merged` → blocker 含 `develop：未合并`；`merged/merged/dirty` → blocker 含 `存在未提交改动`；`merged/merged/!dirty/stale` → blocker 含 `fetch 失败`。
2. `branchMissing`、`notCheckable`、`error` 各产生对应 blocker。

`assess`：

3. 两条 active（一干净一不干净）→ `clean = false`，`records[1].blockers` 非空，`confirmationText == iteration`。
4. 只剩 `removed` 记录 → `records = []`、`clean = true`（与 007 测试 6 共用）。
5. `archive-progress` 事件每条 `record` 阶段一次且 `record == null`。
6. 清单 `damaged` → `ManifestDamaged`；已有 `archivedAt` → `Conflict`。

`archive`：

7. `confirmation = "7.3.0 "`（带空格）→ `Validation`，清单不变。
8. 不干净且 `force = false` → `Validation`，无 Git `worktree remove` 调用，清单不变。
9. 全干净 `force = false` → 每条 `worktree remove <path>`（`GitArgs` 断言不含 `--force`）；清单每条 `lifecycle = removed` 且 `removedAt` 非空；`archivedAt` 非空；`archived = true`、`removedCount == N`；本地分支仍存在（`show-ref` 验证）；迭代目录、公共目录快照、清单文件仍存在。
10. 一干净一不干净 + `force = true` → 干净条不带 `--force`，不干净条带单个 `--force`；两条都 `removed`；`archivedAt` 非空。
11. 一条失败（提前把该 worktree 目录改为只读或用 `worktree lock` 锁定）→ 其余仍被移除、`failed.len() == 1`、`archived = false`、`archivedAt == null`；再次归档（修复后）可成功。
12. `locked` 记录在 `force = true` 下仍失败进 `failed[]`，且 `GitArgs` 中不存在 `--force --force`。
13. 操作锁：持锁期间第二次 `archive_iteration` → `busy`。

`list_groups`：

14. `archivedAt` 非空的迭代在 `reconcile = false` 与 `true` 两档都不出现；同时 `hiddenAt` 非空也不出现（优先级）。

前端 SSR：

15. `ArchiveDialog` 在 `clean = false` 时渲染 blockers 文案与「强制归档」复选框；`clean = true` 时不渲染复选框。
16. 「归档」按钮禁用条件纯函数：`confirmation !== iteration || (!clean && !force)`。

真实窗口手工验证：一个含 2 个 worktree 的迭代归档后从列表消失，`~/…/7.3.0/` 目录与 `.worktree-manager.json` 仍在，源仓库分支仍在。

---

## 6. 已知局限

- **归档不删任何东西**：本地分支、远端分支、迭代目录、公共目录快照、`vendor` 副本、清单全部保留；磁盘空间不会释放。整迭代删除见 `todo.md`。
- **不可撤销**：没有「取消归档」命令；要让迭代重新出现只能手工把清单 `archivedAt` 改回 `null`（此时所有记录已是 `removed`，只能看历史）。
- **强制归档会丢弃未提交改动**：`--force` 的 `worktree remove` 会删除工作区中的修改与未跟踪文件；本侧未推送的提交因分支保留而不丢，但 detached 记录的独立提交只剩 reflog 可找回。
- **`stale` 一律阻塞**：离线环境下无法「干净归档」，只能强制。
- **discovered worktree 不受影响**：归档后它们仍留在迭代目录里，而迭代已从列表消失，用户看不到它们；需要手工处理。
- **部分失败后的中间态**：`archived = false` 时迭代仍在列表里，成功的记录显示「已移除」，失败的仍 `active`，可再次归档；但两次归档之间的评估结果可能不同。
- **评估两次**：`archive` 会重跑 `assess`（含 fetch），对话框里看到的评估与实际执行依据的评估之间存在时间差，若远端在此期间变化，最终判定以执行时为准，用户不会看到第二次评估的明细。
- **继承 001 的判定局限**：squash 后又改同一批文件的误报、revert 后显示未包含等，见 `design/001 §6`。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

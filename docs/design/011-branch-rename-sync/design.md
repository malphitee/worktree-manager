# 011 · 分支改名同步

> 设计文档是**当次决策的历史记录**。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 基线：`docs/requirements.md §3.7 / §3.8 / §3.11`、`docs/data-model.md §3 / §5`、`docs/architecture.md §5 / §6.2`、`docs/implementation-plan.md S7`。

---

## 1. 背景与目标

**现状**：清单在创建时记录 `branch`。用户在 worktree 里执行 `git branch -m old new` 后，源仓库不再有 `old`，worktree 仍注册在同一路径并检出 `new`。复核时清单分支与 live 分支不一致，被判为 `headMismatch`；合并检查按清单名找不到分支报 `branchMissing`；安全移除同样按清单名评估，给出错误结论。改名是正常操作，不应被当成异常。

**目标**：

- 复核时识别「本地改名」：清单分支已不存在 + 同路径注册 + 检出了另一个分支 → 判 `valid`，`branchDisplay` 取 live 名，`renamedFrom` 记旧名，UI 显示「⟳ 已同步改名」。
- 识别后把清单 `branch` **原子回写**为 live 名，只改这一个字段。
- 复核完成后 toast「已同步 N 个分支重命名」（N = 0 不提示）。
- 合并检查与安全移除统一按 live 名判定。
- 「切分支/串台」（清单分支仍存在但 live 不同）仍判 `headMismatch`，清单不动。

**非目标**：

- 不区分「改名」与「删旧建新」（Git 层面不可区分）。
- 不同步远端分支改名（不 `push`、不改 upstream）。
- 不在快扫档做任何判定（快扫不跑 Git）。
- 不持久化 `renamedFrom`。

---

## 2. 已确认决策

1. **判定条件三合一：清单分支不存在 ∧ 同路径注册 ∧ live 分支存在且 ≠ 清单分支。缺任一条件都不算改名。**
   被否决备选：只要清单分支不存在就按 live 名同步。原因：worktree 路径未注册（`notRegistered`）或 live 为 detached 时，「改名」没有证据，盲目同步会把 `detached` 记录改写成错误状态。

2. **只有清单分支「不存在」才同步；清单分支仍存在时一律 `headMismatch`。**
   被否决备选：清单分支存在但 worktree 检出了别的分支时也同步为 live 名。原因：那是「切分支」，用户可能只是临时切过去看看；自动改写清单会让记录跟着用户的临时操作漂移，且无法回退。

3. **回写只改 `branch` 字段，其余字段语义不变；用整份清单重新序列化而不是文本替换。**
   被否决备选：字节级只替换 `branch` 那一行。原因：手写 JSON 局部替换不可靠；`atomic_json` 本来就是整份原子写，只要读→改一个字段→写，其他字段语义等价即可（S7 测试用反序列化后逐字段比对，不做字节比对）。

4. **`renamedFrom` 只在本次复核的投影里返回，不写清单。**
   被否决备选：清单增加 `renamedFrom` / `renameHistory` 字段。原因：改名同步后清单已经是正确状态，历史信息对任何后端判定都无用；引入字段只会增加 schema 负担。

5. **复核（`list_workspaces(reconcile=true)`）持全局操作锁，原因就是本功能会回写清单。**
   被否决备选：复核不持锁、回写时单独加锁。原因：复核跨多个仓库耗时数秒，中途若有创建/移除写同一份清单，回写会覆盖对方的字段；全局锁最简单也最安全。

6. **合并检查（001）与安全移除（MVP）在读清单后先经 `head_state::judge`，用其返回的 live 名做后续判定；但这两条路径**不回写**清单。**
   被否决备选：三条路径都回写。原因：回写只在复核路径做一次即可；合并检查与移除不该有「顺便改清单」的副作用，职责单一、便于测试。它们持锁是因为各自原因（合并检查跑 fetch、移除改清单），与回写无关。

7. **`detached` 记录不参与改名判定：live 是 detached 且 HEAD == `baseCommit` → `valid`；live 是分支 → `headMismatch`；HEAD ≠ `baseCommit` 但仍 detached → `valid`（用户在 detached 上提交了，属于正常「有变更」，由 005 表达）。**
   被否决备选：detached 记录的 HEAD 变了就判 `headMismatch`。原因：detached HEAD 上提交是合法用法（后续可 `git branch` 收编），不应当作异常；能否安全移除由 `detachedCommits` 风险单独保证。

8. **toast 文案固定「已同步 N 个分支重命名」，N = 0 不弹。**
   被否决备选：每条改名各弹一条 toast。原因：批量复核可能同步多条，一条汇总足够，行内已有「⟳ 已同步改名」标签。

---

## 3. 技术方案概要

### 3.1 数据结构

- 输入：`ProjectRecord`（清单记录，`data-model.md §2`）、`WorktreeEntry`（`git.rs::parse_worktree_list` 产物：`path`, `head: String`, `branch: Option<String>`, `detached: bool`, `locked: Option<String>`, `prunable: Option<String>`）。
- 输出：`WorkspaceProject.validity / branchDisplay / renamedFrom`（`data-model.md §5`）。
- 新增内部类型（`head_state.rs`，不序列化）：

```rust
pub enum HeadJudgement {
    /// 清单与 live 一致
    Valid { live_branch: Option<String> },
    /// 本地改名：live 名与清单旧名
    Renamed { live_branch: String, previous: String },
    /// 清单分支仍存在但与 live 不一致，或 headMode 不匹配
    HeadMismatch { live_display: String },
}
```

### 3.2 后端

模块：`head_state.rs`（判定）、`manifest.rs`（复核流程调用 + 回写）、`merge_check.rs` / `removal.rs`（消费 live 名）。

```rust
// head_state.rs
/// 前置：record.lifecycle == active，entry 已按规范化路径匹配到同一 worktree（未匹配到由调用方判 notRegistered）
pub fn judge(record: &ProjectRecord, entry: &WorktreeEntry, git: &GitRunner) -> Result<HeadJudgement, AppError>;

/// 返回参与后续判定的分支名：Renamed → live；Valid → 清单/live（相同）；HeadMismatch → 清单名（不采纳临时切换）
pub fn effective_branch(record: &ProjectRecord, judgement: &HeadJudgement) -> Option<String>;

// manifest.rs（复核路径）
/// 对 Renamed 记录回写 branch；返回同步条数
pub fn sync_renamed_branches(iteration_dir: &Path, manifest: &mut Manifest, renames: &[(usize, String)]) -> Result<usize, AppError>;
```

`judge` 算法（`record.headMode == branch` 时）：

1. `manifest_branch = record.branch`（非空）。
2. `exists = git.show_ref_branch_exists(manifest_branch)` → 参数向量 `show-ref --verify --quiet refs/heads/<manifest_branch>`（退出码 0 = 存在）。
3. `live = entry.branch`（来自 `worktree list --porcelain` 的 `branch refs/heads/<name>` 行；`detached` 行则为 `None`）。
4. 判定：
   - `live == Some(manifest_branch)` → `Valid`。
   - `!exists && live.is_some() && live != manifest_branch` → `Renamed { live_branch: live, previous: manifest_branch }`。
   - `exists && live != manifest_branch`（含 live 为 detached）→ `HeadMismatch { live_display }`。
   - `!exists && live.is_none()`（清单分支没了且 worktree 是 detached）→ `HeadMismatch`（无改名证据）。

`record.headMode == detached` 时：

- `entry.detached && entry.head == record.baseCommit` → `Valid`。
- `entry.detached && entry.head != record.baseCommit` → `Valid`（见决策 7）。
- `entry.branch.is_some()` → `HeadMismatch { live_display: <branch> }`。

Git 调用序列（复核路径，每条 `active` 记录）：

1. `worktree list --porcelain`（按源仓库缓存，每仓库一次，由 `manifest.rs` 复核流程执行，不在 `judge` 内）。
2. `show-ref --verify --quiet refs/heads/<清单branch>`（仅 `headMode == branch`）。
3. 后续 `status --porcelain=v1 --untracked-files=all`、`rev-list --count <base>..HEAD` 由复核流程继续执行，与本功能无关。

复核流程集成（`manifest.rs::list_groups(reconcile=true)`）：

1. 读清单；损坏/缺失照常报健康度，跳过判定。
2. 逐条 `active` 记录：目录不存在 → `missingDirectory`；未匹配到 entry → `notRegistered`；源仓库不可用 → `sourceMissing`；否则 `judge`。
3. `Renamed` → `validity = valid`、`branchDisplay = live`、`renamedFrom = previous`，收集 `(index, live)`。
4. 全部记录处理完后，若收集非空 → `sync_renamed_branches`（改 `manifest.projects[i].branch`，`atomic_json::write_json_atomic`），失败则该迭代 `manifestMessage` 记录「分支重命名回写失败：<原因>」，投影仍按 live 名返回（下次复核会再试）。
5. 计数汇总到返回值：`WorkspaceGroup` 不新增字段；`list_workspaces` 命令的返回类型不变，同步计数通过每个 `WorkspaceProject.renamedFrom` 非空的个数由前端汇总（决策：避免为一个 toast 改命令签名）。

合并检查（`merge_check.rs`）与移除（`removal.rs`）：读清单 + `worktree list` 后先 `judge`，用 `effective_branch` 得到分支名；`Renamed` 时不回写。`HeadMismatch` 时合并检查按清单名判（分支存在），移除路径按 `pathInvalid` 之外的常规风险评估（`headMismatch` 记录允许打开移除对话框，见 `data-model.md §5 removable`）。

### 3.3 前端

- 入口组件：`WorkspaceList.vue` 的「复核状态」按钮 → `listWorkspaces(true)`。
- 完成后：`renamedCount = groups.flatMap(g => g.projects).filter(p => p.renamedFrom).length`；`> 0` 时 toast（success）「已同步 N 个分支重命名」。
- `IterationCard.vue` 的 `project` 列：`renamedFrom` 非空时在 `projectId` 后追加 `.tag.success` 标签「⟳ 已同步改名」，`title` 为「原分支：<renamedFrom>」。
- 状态：`groups` 存 `App.vue`（页面级），整体替换；`renamedFrom` 只是投影字段，下次快扫后自然消失。
- `api/tauri.ts`：无新函数（复用 `listWorkspaces`）。

### 3.4 事件

无。

---

## 4. 状态与枚举

| 枚举 | 取值 | 本功能含义 |
| --- | --- | --- |
| `Validity` | `valid` | 一致，或本地改名已同步 |
| `Validity` | `headMismatch` | 清单分支仍存在但检出了别的分支 / detached 记录被检出了分支 / 清单分支不存在且 worktree 为 detached |
| `Validity` | `notRegistered` | 源仓库 `worktree list` 无该路径（不进入改名判定） |
| `Validity` | `missingDirectory` / `sourceMissing` | 不进入改名判定 |
| `HeadMode` | `branch` / `detached` | 决定走哪套判定 |
| `MergeCellStatus` | `branchMissing` | 改名同步后**不应**再出现于已同步记录；仅当 live 也不存在（分支被删）时出现 |
| `WorkspaceProject.renamedFrom` | `string \| null` | 非空＝本次复核同步过 |

---

## 5. 测试要点

Rust（S7，`tempfile` + 裸仓库 fixture）：

- **改名 fixture**：创建 worktree 到 `feature/a` → 在 worktree 内 `git branch -m feature/a feature/b` → 复核：`validity = valid`、`branchDisplay = "feature/b"`、`renamedFrom = "feature/a"`；清单 `branch == "feature/b"`；清单其他字段（反序列化后逐字段）不变；临时文件不残留。
- **切分支 fixture**：源仓库另建 `feature/c`，worktree 内 `git checkout feature/c`（`feature/a` 仍存在）→ `headMismatch`、`branchDisplay = "feature/c"`、`renamedFrom = null`；清单 `branch` 仍 `feature/a`。
- **清单分支删了且 worktree detached**：`git checkout --detach` 后 `git branch -D feature/a` → `headMismatch`，清单不变。
- **detached 记录**：HEAD == `baseCommit` → `valid`；在 detached 上 `commit` → `valid`；`checkout -b x` → `headMismatch`。
- **改名后合并检查**：`check_merge_status` 对该记录不返回 `branchMissing`，按 `feature/b` 判定。
- **改名后移除评估**：`assess_removal` 不因分支名报 `pathInvalid`/`manifestInvalid`；风险按 `feature/b` 计算。
- **回写失败**：迭代目录只读时复核仍返回投影（`valid` + `renamedFrom`），`manifestMessage` 含「回写失败」，清单文件不变。
- **二次复核幂等**：同步后再复核 → `renamedFrom = null`、清单不再写入（mtime 不变）。
- **操作锁**：复核期间 `set_iteration_note` 返回 `busy`。
- `judge` 单测：用构造的 `ProjectRecord` + `WorktreeEntry` + 可注入的 `show-ref` 结果覆盖 §3.2 全部分支。

前端（`scripts/test-frontend.mjs`）：

- `IterationCard` 传入 `renamedFrom: "feature/a"` → SSR 输出含「已同步改名」；`renamedFrom: null` → 不含。

真实窗口：改名后点「复核状态」→ 行内标签 + toast「已同步 1 个分支重命名」；再点一次 → 无 toast。

---

## 6. 已知局限

- 改名后若又在源仓库建了同名新分支（`feature/a` 再次存在），判定条件「清单分支不存在」不满足，会判成 `headMismatch`；无法区分「改名」与「切分支」。
- `renamedFrom` 只在本次复核的返回里出现，不落盘；刷新快扫后标签消失。
- 一次复核只同步一次：若在复核过程中用户再次改名，要等下次复核。
- 「删旧分支 + 新建分支 + checkout」与真正的 `branch -m` 在 Git 层面不可区分，都会被当作改名同步。
- 远端分支不同步：改名后远端仍是旧名，`push` 需要用户自己处理；合并检查比较的是 live 本地分支与 `origin/develop|master`，不受影响。
- 回写失败只在 `manifestMessage` 提示，不重试、不回滚。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

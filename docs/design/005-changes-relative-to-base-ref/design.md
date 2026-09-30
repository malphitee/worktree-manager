# 005 · 相对基分支的「有变更」判定

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 关联：`requirements.md §3.6`、`data-model.md §4/§5`、`architecture.md §5`、`implementation-plan.md S5`。

---

## 1. 背景与目标

### 现状

列表「基准变动」列需要回答「这个 worktree 相对它创建时的基分支，是否有本侧独有的提交」。最直观的做法是拿清单里的 `baseCommit`（创建时 `FETCH_HEAD` 解析出的不可变 Commit）与当前 HEAD 比较：不相等即「有变更」。这个做法在两种常见场景下误报：

- 用户在 worktree 里 `rebase` 到基分支的新 tip，HEAD 变了但没有任何本侧提交；
- 基分支自身前进（别人推了提交），用户 `merge` 进来，HEAD 变了但依然没有本侧独有提交。

### 目标

- `hasChanges` 定义为：HEAD 相对「创建时记录的基分支 `baseRef`」存在本侧独有提交，即 `rev-list --count <base>..HEAD > 0`。
- 复核档（`list_workspaces(reconcile=true)`）与合并检查（001/003）共用同一个判定函数，保证两条路径给出的结论一致。
- discovered 行（无清单记录）也能给出一个可解释的结论。

### 非目标

- 不判断本侧提交是否已推送（那是移除风险 `unpushedCommits` 的事）。
- 不判断是否已合并进 `develop`/`master`（那是 001 合并矩阵的事）。
- 不修改清单；本功能只读。
- 不联网：本功能不执行 `fetch`，只用本地已有引用。

---

## 2. 已确认决策

1. **比较基准是 `baseRef` 解析出的引用，不是 `baseCommit`。**
   被否决备选：用 `baseCommit` 与 HEAD 比较。原因：rebase 或基分支前进后 HEAD 必然变化，即使没有本侧提交也会误报「有变更」。`baseCommit` 仍保留在清单里作为创建时的审计信息，只是不参与本判定。
2. **解析链固定四级，取第一个可解析者，都不可解析则 `hasChanges = null`。**
   顺序：① `baseRef` 原文（形如 `origin/release`）→ ② 去掉 remote 前缀的本地分支名（`release`）→ ③ `origin/master` → ④ `master`。
   被否决备选：只用 `baseRef`，解析失败即报错。原因：远端跟踪引用可能被用户 `git remote prune` 掉，或旧清单记录的 remote 已改名；给一个退化但可解释的结论比整行报错更有用。
3. **判定命令是 `rev-list --count <base>..HEAD`，大于 0 即 `true`。**
   被否决备选：`git log --oneline <base>..HEAD` 再数行。原因：`rev-list --count` 输出单个整数，解析最简单、最不易被 subject 中的换行干扰；且已在 `architecture.md §5` 白名单内。
4. **discovered 行固定用 `origin/master`，不存在则 `master`，再不存在则 `null`。**
   被否决备选：discovered 行不给结论（恒 `null`）。原因：discovered 行同样出现在列表里，用户需要一个粗略提示；`origin/master` 是本产品里合并检查的固定目标之一，作为退化基准语义清楚。
5. **复核档与合并检查调用同一个函数 `head_state::has_changes`（放在 `head_state.rs` 或 `merge_check.rs` 均可，但只能有一处实现）。**
   被否决备选：合并检查内部另写一套。原因：两处实现会漂移，列表在「复核后」与「合并检查后」显示不同结论会让用户困惑（006 依赖两者一致）。
6. **`dirty` 与 `hasChanges` 分开：`dirty` 只看 `status --porcelain` 是否非空，与本判定无关。**
   被否决备选：`dirty` 为真时也把 `hasChanges` 置真。原因：未提交改动不在任何 commit 里，`rev-list` 不可能看到；混在一起会让「基准变动」列语义不清。列表用两个独立标签展示（见 `ui-spec.md §4.1`）。

---

## 3. 技术方案概要

### 数据结构（引用 `data-model.md`）

- `WorkspaceProject.hasChanges: boolean | null`、`WorkspaceProject.dirty: boolean | null`、`WorkspaceProject.baseRef: string | null`。
- `MergeRecordResult.hasChanges: boolean | null`、`MergeRecordResult.dirty: boolean | null`（由 006 消费）。
- 清单 `projects[].baseRef: string`（只读取，不写）。

本功能**不新增**任何持久化字段。

### 后端

模块：`src-tauri/src/head_state.rs`（与 011 同模块，因为都属于「live 状态判定」），由 `manifest.rs` 的复核路径与 `merge_check.rs` 共同调用。

```rust
/// 基准引用解析结果。
pub struct ResolvedBase {
    /// 实际使用的引用名，如 "origin/release"、"release"、"origin/master"、"master"
    pub reference: String,
    /// 解析链命中的层级，1..=4，用于日志与测试
    pub level: u8,
}

/// 按解析链找到第一个可解析的基准引用；全部失败返回 None。
/// base_ref 为 None 表示 discovered 行（只尝试 origin/master、master）。
pub fn resolve_base(git: &GitRunner, base_ref: Option<&str>) -> Result<Option<ResolvedBase>, AppError>;

/// HEAD 相对基准是否有本侧独有提交。base 为 None 时返回 Ok(None)。
pub fn has_changes(git: &GitRunner, base: Option<&ResolvedBase>) -> Result<Option<bool>, AppError>;

/// 未提交改动：status --porcelain 非空。
pub fn is_dirty(git: &GitRunner) -> Result<bool, AppError>;
```

`GitRunner` 的 `repo` 指向 **worktree 目录**（不是源仓库根），因为 `HEAD` 与 `status` 都是 worktree 局部的；`rev-list` 与 `rev-parse` 在 worktree 内执行同样能看到共享的 refs。

「去掉 remote 前缀的本地名」的规则：`baseRef` 按第一个 `/` 切分，若首段是 `git remote` 列表中的某个 remote 名，则取剩余部分作为本地分支名；否则跳过第二级（`baseRef` 本身可能就是裸分支名，此时第一级已尝试）。

#### Git 调用序列（参数向量按 `architecture.md §5`，按执行顺序）

对每一行：

1. `remote` —— 获取 remote 名集合（每个源仓库缓存一次，复核与合并检查同批内复用）。
2. 逐级尝试 `rev-parse --verify <candidate>^{commit}`，候选依次为 `baseRef`、本地名、`origin/master`、`master`；第一个退出码为 0 的即 `ResolvedBase`。
   > 注：白名单里 `rev-parse --verify` 形式已存在（`FETCH_HEAD^{commit}`），此处只是换引用名，不算新增子命令。
3. `rev-list --count <base>..HEAD` —— 解析 stdout 为 `u64`，`> 0` → `true`。
4. `status --porcelain=v1 --untracked-files=all` —— 非空 → `dirty = true`（复核路径本来就要跑，合并检查路径复用同一函数）。

worktree 目录不存在（`validity = missingDirectory`）时直接返回 `hasChanges = null`、`dirty = null`，不调用 Git。

### 前端

- 入口组件：`WorkspaceList.vue` / `IterationCard.vue` 的 `dirty` 列（表头「基准变动」）。
- `api/tauri.ts`：无新增函数；值通过 `listWorkspaces(true)` 与 `onMergeCheckProgress` 的 `record` 到达。
- 状态存放：`WorkspaceProject.hasChanges/dirty` 随 `groups` 存于 `App.vue`；合并检查覆盖值存于 `WorkspaceList.vue` 内的 `Map<iteration, MergeCheckResult>`（见 006）。
- 展示映射（`utils/status.ts`）：`hasChanges === true` → 「有变更」warning；`false` → 「无」neutral；`null` → 「未复核」neutral；`dirty === true` 额外附「未提交」danger。

### 事件

无新增事件。合并检查的 `merge-check-progress`（`phase = record`）载荷里的 `record.hasChanges/dirty` 由本函数产生（见 006）。

---

## 4. 状态与枚举

| 字段 | 取值 | 含义 |
| --- | --- | --- |
| `hasChanges` | `true` | `rev-list --count <base>..HEAD > 0` |
| | `false` | 计数为 0 |
| | `null` | 快扫档未复核；或目录缺失；或四级解析链全部失败 |
| `dirty` | `true` | `status --porcelain` 非空 |
| | `false` | 空 |
| | `null` | 快扫档未复核；或目录缺失 |
| `ResolvedBase.level` | `1..=4` | 命中 `baseRef` / 本地名 / `origin/master` / `master` |

本功能不引入新的 `Validity` 或 `MergeCellStatus` 取值。

---

## 5. 测试要点

（对应 `implementation-plan.md S5`，全部用 `tempfile` + 本地裸仓库；本机无 `git` 时打印原因并 `return`。）

1. **rebase 不误报**：从 `origin/release` 创建分支 → 远端 `release` 前进一个提交 → `fetch` → 本地 `rebase origin/release` → `hasChanges = false`（`baseCommit` 已不等于 HEAD，用于反证决策 1）。
2. **基分支前进 + merge 不误报**：同上改为 `merge origin/release` → `hasChanges = false`。
3. **真实本侧提交**：本地新建一个提交 → `hasChanges = true`。
4. **解析链第二级**：`git update-ref -d refs/remotes/origin/release` 删除远端跟踪引用，但本地存在 `release` 分支 → `level = 2`，结论按 `release` 计算。
5. **解析链第三级**：`baseRef = origin/nonexistent`，无同名本地分支，存在 `origin/master` → `level = 3`。
6. **解析链第四级**：只存在本地 `master` → `level = 4`。
7. **全部失败**：仓库无任何候选引用 → `hasChanges = null`，不报错。
8. **discovered 行**：`base_ref = None` 且存在 `origin/master` → 按其计算；不存在但有 `master` → `level = 4`；都无 → `null`。
9. **detached HEAD**：detached 记录同样按 HEAD commit 计算，不因无分支名而报错。
10. **dirty 独立**：工作区有未提交改动但无本侧提交 → `dirty = true`、`hasChanges = false`。
11. **目录缺失**：worktree 目录被删除 → `hasChanges = null`、`dirty = null`，且不产生 Git 调用（用 `GitArgs` 记录器断言）。
12. **参数向量**：`rev-list --count <base>..HEAD` 的 `Vec<String>` 逐项比对。
13. **一致性**：同一 fixture 下复核档与合并检查返回的 `hasChanges/dirty` 相等。

---

## 6. 已知局限

- **本地基分支落后于远端时显示「有变更」**：本功能不 `fetch`，若 `origin/release` 的本地跟踪引用落后，而用户的分支已经包含远端更新的提交，`rev-list` 会把这些提交算成「本侧独有」。这是刻意取舍：保持本功能离线、快速；远端真相由合并检查（001）负责，且 006 会用合并检查的结果覆盖列表值。
- **解析链退化到 `origin/master`/`master` 时语义变弱**：此时「有变更」的含义变成「相对 master 有独有提交」，与创建时的基分支无关。UI 不区分命中层级（`level` 只在日志与测试中使用）。
- **只看 commit 图**：未提交改动、stash、未跟踪文件都不在任何 commit 里，本判定看不到；`dirty` 单独展示。
- **`baseRef` 记录的 remote 被改名后**第一级必然失败，退到第二级或更低。
- **discovered 行的基准是猜的**：无清单记录，只能假定 `origin/master`。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

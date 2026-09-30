# 007 · 合并检查只查 active 记录

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 关联：`requirements.md §3.7`、`data-model.md §2/§3/§4`、`design/001`、`design/003`、`implementation-plan.md S5`。

---

## 1. 背景与目标

### 现状

迭代清单 `projects[]` 里有三种 `lifecycle`：`active`（正常托管）、`removed`（已安全移除或已归档，目录不在了但记录保留作历史）、`createFailed`（创建失败，目录可能根本没建）。合并检查（001）如果对全部记录逐条跑 Git，`removed`/`createFailed` 记录要么目录不存在（`notCheckable`），要么本地分支已删（`branchMissing`），既浪费时间、又在事件流里产生一堆无意义的行，前端还得决定这些行往哪放。

列表页只渲染 `active` 记录（快扫与复核的投影都会把 `removed` 显示为「已移除」一行，但那一行没有可操作的合并格）。

### 目标

- `check_merge_status` 只对 `lifecycle = active` 的记录跑判定与推送事件；`records[]` 与 `total` 都只计 active。
- 合并检查的行集合与列表可见、可操作的行集合完全一致，前端不需要过滤。

### 非目标

- 不改变 `removed`/`createFailed` 记录在列表里的展示。
- 不对 discovered 行做合并检查。
- 不提供「包含已移除记录」的开关。

---

## 2. 已确认决策

1. **过滤条件固定为 `lifecycle == active`，在读清单后、发出任何 `fetching` 事件前完成；`total` 等于过滤后的条数。**
   被否决备选：也查 `removed` 记录，把结果展示为「历史合并状态」。原因：`removed` 记录的 worktree 目录不存在，只能按清单里的分支名在源仓库判定；而该分支可能已被用户删除、也可能被重建成别的内容，结论不可靠；并且这些行在列表里没有合并格可以承载结果。归档（008）需要的是「归档前」的判定，那时记录还是 `active`。
2. **`createFailed` 同样排除。**
   被否决备选：对 `createFailed` 且 `baseCommit` 非空的记录判定。原因：`createFailed` 记录没有 worktree、通常也没有分支（`worktree add -b` 失败即无分支），判定结果只能是 `branchMissing`/`notCheckable`，无信息量。
3. **discovered 行不查。**
   原因：discovered 行没有清单记录，`check_merge_status` 以迭代清单为输入；要查 discovered 需要先复核（才知道它存在）再把路径传回后端，后端又要重新验证该路径是否在迭代目录内且是 worktree。这条路径的安全校验与收益不成比例。用户若想检查，先把它移除或手工纳入清单（本产品不提供「纳入」操作，见 `todo.md`）。
   被否决备选：`check_merge_status` 接受任意 `worktreePath` 列表。原因同上，且违反「前端提交的路径不可信、目标路径由后端逐段 join」的信任模型（`architecture.md §4.2`）。
4. **可选参数 `projectId`/`worktreePath` 用于单行重查，但仍受 `active` 过滤约束**：指定的记录若不是 `active`，返回 `Validation` 错误「该记录不是活动 worktree」，而不是静默返回空 `records`。
   被否决备选：静默返回空数组。原因：前端无法区分「没有匹配记录」与「记录不活跃」，会显示一个空转的 loading。
5. **008 归档评估复用同一过滤**：`assess_archive` 的 `records[]` 同样只含 `active`；`clean` 只由 active 记录决定；一个只剩 `removed` 记录的迭代 `clean = true`。
   被否决备选：无 active 记录时拒绝归档。原因：用户可能已逐条手动移除完，归档只是为了让迭代从列表消失，这是合理需求。

---

## 3. 技术方案概要

### 数据结构（引用 `data-model.md`）

- 清单 `projects[].lifecycle: Lifecycle`（读取）。
- `MergeCheckResult.records[]: MergeRecordResult`，其中 `lifecycle` 字段恒为 `active`（保留该字段是为了结构对称与将来扩展，前端不依赖它过滤）。
- `MergeCheckProgress.total`：过滤后的 active 条数；`index` 为 0-based 顺序号，范围 `[0, total)`。

不新增字段。

### 后端

模块：`src-tauri/src/merge_check.rs`。

```rust
pub struct MergeCheckFilter<'a> {
    pub project_id: Option<&'a str>,
    pub worktree_path: Option<&'a str>,
}

/// 读清单 → 过滤 active → 按源仓库分组 fetch → 逐条判定并推送事件。
pub fn check(
    config: &AppConfig,
    iteration: &str,
    filter: MergeCheckFilter<'_>,
    emit: &dyn Fn(MergeCheckProgress),
) -> Result<MergeCheckResult, AppError>;

/// 过滤函数单独暴露，供 archive.rs 与单测复用。
pub fn active_records<'m>(manifest: &'m Manifest) -> Vec<&'m ManifestProject>;
```

`active_records` 保持清单数组顺序（014 的展示顺序即检查顺序）。

`filter` 非空时：在 `active_records` 结果里按 `worktree_path`（优先）或 `project_id` 匹配；匹配不到则检查全清单——若存在但非 `active` 返回 `Validation("该记录不是活动 worktree")`，若根本不存在返回 `NotFound`。

#### Git 调用序列（参数向量按 `architecture.md §5`，按执行顺序）

过滤本身不调用 Git。对过滤后的每个**源仓库**（去重）：

1. `fetch --no-tags origin develop master`（失败 → 该仓库 `stale = true`，继续）

对过滤后的每条记录（见 `design/001`）：

2. `worktree list --porcelain`（按源仓库缓存）
3. `show-ref --verify --quiet refs/heads/<branch>` / `symbolic-ref -q --short HEAD` / `rev-parse --verify HEAD`（确定 live 名或 detached commit，011）
4. `merge-base --is-ancestor <branch> origin/<target>` × 2
5. `cherry origin/<target> <branch>` × ≤2
6. `merge-tree --write-tree origin/<target> <branch>` + `rev-parse origin/<target>^{tree}` × ≤2
7. 005/006 的 `rev-parse --verify` / `rev-list --count` / `status --porcelain=v1 --untracked-files=all`

`removed`/`createFailed` 记录**零 Git 调用**（测试用 `GitArgs` 记录器断言）。

### 前端

- 入口组件：`IterationCard.vue` 的「检查合并」按钮；`MergeDetailDialog.vue` 的「重新检查此行」（可选，调用带 `worktreePath` 的单行版本）。
- `api/tauri.ts`：`checkMergeStatus(iteration, projectId?, worktreePath?)`。
- 状态存放：同 006，`WorkspaceList.vue` 的 `mergeResults`。
- 前端**不做**任何 `lifecycle` 过滤：收到的 `record` 直接按 `worktreePath` 合并进行渲染；`removed` 行永远不会收到 record，其合并格固定渲染为「—」。

### 事件

复用 `merge-check-progress`。`total` 为 active 条数；无 active 记录时不发任何事件，命令直接返回 `records = []`。

---

## 4. 状态与枚举

| 枚举 | 取值 | 本功能中的处理 |
| --- | --- | --- |
| `Lifecycle` | `active` | 参与检查、出现在 `records` 与事件流 |
| | `removed` | 跳过；列表合并格显示「—」 |
| | `createFailed` | 跳过；列表合并格显示「—」 |
| `Validity` | `discovered` | 不参与（无清单记录） |
| `MergeCellStatus` | 全部取值 | 仅对 active 记录产生 |

---

## 5. 测试要点

（`tempfile` + 裸仓库，`implementation-plan.md S5`。）

1. 清单含 `active` ×2、`removed` ×1、`createFailed` ×1 → `records.len() == 2`，事件 `total == 2`，`phase = record` 事件 2 次，且 `records[].lifecycle` 全为 `active`。
2. 同上，用 `GitArgs` 记录器断言：不存在以 `removed`/`createFailed` 记录的 `worktreePath` 为工作目录的 Git 调用。
3. `active_records` 保持清单数组原顺序（构造乱序 fixture 验证）。
4. 只有 `removed` 记录的迭代 → `records = []`、无事件、不报错。
5. `filter.worktree_path` 指向 `removed` 记录 → `AppError::Validation`；指向不存在路径 → `AppError::NotFound`；指向 `active` 记录 → `records.len() == 1`、`total == 1`。
6. `assess_archive` 对「只剩 removed 记录」的迭代返回 `clean = true`、`records = []`（与 008 共用 fixture）。
7. 前端 SSR：`IterationCard` 对 `lifecycle = removed` 的行渲染合并格为「—」，且不带可点击的详情按钮。

---

## 6. 已知局限

- **已移除记录的历史合并状态不可见**：用户若想知道某个已移除分支是否合并过，只能去源仓库查。归档场景下这不是问题，因为归档前的评估已经展示过。
- **discovered worktree 不参与合并检查**，也不参与归档 clean 判定；归档时 discovered 目录会原样留在迭代目录里（008 不动它们）。
- **`index/total` 是过滤后的编号**，与清单数组下标不对应；前端只用它显示进度，不用于定位行。
- 单行重查（`filter` 非空）仍会对该源仓库执行一次 `fetch`，无法只做离线判定。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

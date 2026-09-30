# 001 · 迭代/分支合并状态检查

> 设计文档是**当次决策的历史记录**，设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 相关基线：`docs/requirements.md §3.7`、`docs/architecture.md §5 §6.2 §7`、`docs/data-model.md §4 §5`、`docs/ui-spec.md §4.1 §6`。

---

## 1. 背景与目标

### 现状

MVP 完成后（S3/S4），列表能显示每个 worktree 的存在性与 HEAD 一致性，但**无法回答「这条改动是否已经进了 `develop` / `master`」**。开发者只能逐仓库手工跑 `git branch --contains` 或看远端 PR 状态，10+ 个仓库时成本很高，也是移除/归档前最容易漏判的一步。

### 目标

- 对一个迭代内每条 `lifecycle = active` 的记录，给出 `develop`、`master` 两个目标分支的合并状态矩阵。
- 判定要能识别三种常见合并方式：普通 merge、rebase 后 fast-forward / merge、squash merge。
- 单仓库网络失败不拖垮整批，结果标记「可能过时」后继续。
- 结果是内存快照（带 `checkedAt`），供列表标签、合并详情模态、008 归档评估复用。

### 非目标

- 不配置化目标分支（固定 `origin/develop` + `origin/master`）。
- 不检查 `removed` / `createFailed` 记录（见 007）。
- 不落盘、不缓存跨会话结果。
- 不做「自动合并」「自动删分支」「打开 PR」。
- 不检查未提交改动是否被合并（未提交改动不在任何分支中，只由 `dirty` 单独标出）。

---

## 2. 已确认决策

1. **三层判定，前者命中即止**：① `merge-base --is-ancestor` → `merged`；② `cherry` 无 `+` 行 → `merged`；③ `merge-tree --write-tree` 结果树等于目标树 → `contained`；都不通过 → `unmerged`。
   - 被否决：只用 `--is-ancestor`。原因：rebase 与 squash 都会让祖先判定失败，误报率过高。
   - 被否决：只用 `cherry`。原因：squash 合并后的 patch-id 与原始多个 commit 都不同，无法识别。
   - 被否决：用 `git log --grep` 匹配 commit message。原因：不可靠、依赖团队规范。
2. **对比对象是 fetch 后的远端跟踪引用 `refs/remotes/origin/<target>`，不用本地 `develop`/`master`**。
   - 被否决：用本地分支。原因：本地分支可能落后数周，判定失去意义。
3. **每仓库只 fetch 一次**：`fetch --no-tags origin develop master`；失败则降级用本地 remote-tracking 缓存，该仓库全部单元格 `stale = true` 并继续。
   - 被否决：fetch 失败即整批失败。原因：一个仓库断网不应让其余 9 个仓库的结果作废。
   - 被否决：每个目标分支各 fetch 一次。原因：翻倍网络往返，无收益。
4. **仓库缺目标分支 → 该单元格 `targetMissing`，不整体失败**。
   - 被否决：报错。原因：Go 仓库常常只有 `master` 没有 `develop`，属于正常情况。
5. **分支名来源按 live 名**（引用 011）：`branch` 模式先经 `head_state::judge`，改名后用 live 名，不再报 `branchMissing`。`detached` 记录直接用清单记录目录的 HEAD commit。
   - 被否决：只用清单 `branch` 字段。原因：011 之后清单名可能过期。
6. **只查 `lifecycle = active`**（引用 007）。
7. **结果为内存快照，带 `checkedAt`，不落盘**；前端存 `Map<iteration, MergeCheckResult>`（`WorkspaceList.vue` 内 `ref`）。
   - 被否决：写进清单。原因：清单是「创建时事实」，合并状态是随时变化的外部事实，混在一起会误导且增加写盘风险。
8. **命令持全局操作锁**（`OperationState::acquire("检查合并")`）。
   - 被否决：不持锁（只读操作）。原因：会跑 fetch 且耗时长，与创建/移除并发会让结果与磁盘状态错位；且 008 复用本流程。
9. **Git 子进程在 `spawn_blocking` 中执行**，命令为 `async`，不阻塞 UI。
10. **合并详情模态 `MergeDetailDialog` 只展示，不提供任何 Git 操作按钮**。

---

## 3. 技术方案概要

### 3.1 数据结构（`data-model.md §4`）

使用：`MergeCheckResult { iteration, checkedAt, records[] }`、`MergeRecordResult { projectId, branchDisplay, worktreePath, lifecycle, develop, master, hasChanges, dirty }`、`MergeCellResult { status, unmergedCommits, errorMessage, stale, dirty }`。

本功能新增的字段：以上全部（`hasChanges` / `dirty` 的填充规则见 005/006）。

`unmergedCommits[]` 元素格式：`"<7 位短 hash> <subject>"`，来源于层 2 `cherry` 输出中的 `+` 行，逐条 `git log -1 --format=%h %s <hash>`（见 3.3 补充子命令）。

### 3.2 后端

模块：`src-tauri/src/merge_check.rs`；命令适配在 `lib.rs::check_merge_status`。

```rust
/// 目标分支固定，不配置化
pub const TARGETS: [&str; 2] = ["develop", "master"];

pub struct MergeFilter { pub project_id: Option<String>, pub worktree_path: Option<String> }

/// 入口：对迭代内 active 记录逐条判定，逐条通过 emit 推送
pub fn check(
    config: &AppConfig,
    iteration: &str,
    filter: MergeFilter,
    emit: &mut dyn FnMut(MergeCheckProgress),
) -> Result<MergeCheckResult, AppError>;

/// 每仓库一次；Err 时调用方标记 stale 并继续
fn fetch_targets(git: &GitRunner) -> Result<(), AppError>;

/// 记录 → 判定对象（live 分支名或 HEAD commit）
enum Subject { Branch(String), Commit(String) }
fn resolve_subject(record: &ManifestProject, live: &HeadState) -> Result<Subject, MergeCellStatus>;

/// 单元格三层判定
fn judge_cell(git: &GitRunner, subject: &Subject, target: &str, stale: bool) -> MergeCellResult;

/// 层 2：解析 cherry 输出，返回未包含的 commit hash 列表
fn parse_cherry(stdout: &str) -> Vec<String>;
```

判定算法（`judge_cell`）：

```text
target_ref = "origin/<target>"
0. rev-parse --verify --quiet refs/remotes/origin/<target>^{commit}
   失败 → status = targetMissing，结束
subject_ref = 分支名 或 commit hash
1. merge-base --is-ancestor <subject_ref> <target_ref>
   退出码 0 → merged
   退出码 1 → 继续
   其他退出码 → error（errorMessage = 脱敏后 stderr）
2. cherry <target_ref> <subject_ref>
   输出按行解析：以 "+ " 开头的行为「未被包含」，以 "- " 开头的行为「已包含（等价 patch）」
   无 "+ " 行 → merged
   有 → 记录 plus_hashes，继续
3. merge-tree --write-tree <target_ref> <subject_ref>
   退出码 0：stdout 首行是结果树 OID
       rev-parse <target_ref>^{tree} 得到 target_tree
       OID == target_tree → contained
       否则 → unmerged
   退出码 1（冲突）→ unmerged
   其他退出码 → error
unmerged 时：unmergedCommits = plus_hashes.map(|h| log -1 --format=%h %s h)
```

流程（`check`）：

```text
1. 读清单（health ≠ valid → ManifestDamaged 错误）
2. records = manifest.projects.filter(lifecycle == active).filter(filter 匹配)
3. 按 sourceRepository 分组，保持 records 原顺序；total = records.len()
4. 对每个仓库：
   emit(fetching, "git fetch --no-tags origin develop master")   ← 该仓库第一条记录的 index
   stale = fetch_targets().is_err()
   对该仓库每条记录：
     emit(checking, "git merge-base --is-ancestor …")
     若 worktree 目录不存在 → 两格 notCheckable
     否则 live = head_state::judge(...)；subject = resolve_subject
       branch 模式且 live 分支不存在 → 两格 branchMissing
     develop = judge_cell(..., "develop", stale); master = judge_cell(..., "master", stale)
     dirty = status --porcelain 非空（在 worktree 目录执行）；hasChanges 见 005
     emit(record, message, Some(record))
5. 返回 MergeCheckResult { checkedAt = now, records }
```

### 3.3 Git 调用序列（`architecture.md §5` 参数向量原文，按执行顺序）

| 序 | 参数向量 | 工作目录 | 用途 |
| --- | --- | --- | --- |
| 1 | `fetch --no-tags origin develop master` | 源仓库根 | 每仓库一次 |
| 2 | `worktree list --porcelain` | 源仓库根 | 011 live 判定（每仓库缓存一次） |
| 3 | `show-ref --verify --quiet refs/heads/<branch>` | 源仓库根 | 清单分支是否存在 |
| 4 | `rev-parse --verify HEAD`、`symbolic-ref -q --short HEAD` | worktree 目录 | live HEAD / 分支 |
| 5 | `rev-parse --verify --quiet refs/remotes/origin/<target>^{commit}` | 源仓库根 | 目标是否存在（`rev-parse --verify` 白名单形式的应用） |
| 6 | `merge-base --is-ancestor <subject> origin/<target>` | 源仓库根 | 层 1 |
| 7 | `cherry origin/<target> <subject>` | 源仓库根 | 层 2 |
| 8 | `merge-tree --write-tree origin/<target> <subject>` | 源仓库根 | 层 3 |
| 9 | `rev-parse origin/<target>^{tree}` | 源仓库根 | 层 3 比较 |
| 10 | `log -1 --format=%h %s <hash>` | 源仓库根 | 未合并 commit 展示文案 |
| 11 | `status --porcelain=v1 --untracked-files=all` | worktree 目录 | `dirty` |
| 12 | `rev-list --count <base>..HEAD` | worktree 目录 | `hasChanges`（005） |

序 10 `log -1 --format=%h %s <hash>` 与目标分支存在性判定用的 `rev-parse --verify --quiet <ref>^{commit}` 均已列入 `architecture.md §5` 白名单（偏离 D8）。

### 3.4 前端

- 入口：`IterationCard.vue` 头部「检查合并」按钮 → `api/tauri.ts::checkMergeStatus(iteration)`；订阅 `onMergeCheckProgress`（003）。
- 状态：`WorkspaceList.vue` 内 `mergeResults = ref(new Map<string, MergeCheckResult>())`；`IterationCard` 通过 prop 接收本迭代结果。
- 列：`mergeDevelop` / `mergeMaster` 单元格用 `utils/merge.ts` 映射为标签；`stale` 附「可能过时」；点击单元格打开 `MergeDetailDialog`（props：`projectId`、`branchDisplay`、`target`、`cell: MergeCellResult`），展示 `status` 文案、`unmergedCommits` 列表（mono）、`errorMessage`（若有）。
- 清除时机：014 排序/移动成功后清掉源与目标迭代；008 归档成功后清掉该迭代；其他操作不清（快扫不影响内存结果）。

### 3.5 事件

`merge-check-progress`（结构与时序见 003）。

---

## 4. 状态与枚举

| 枚举 | 值 | 含义 |
| --- | --- | --- |
| `MergeCellStatus` | `merged` | 层 1 或层 2 命中 |
| | `contained` | 层 3 命中（疑似 squash） |
| | `unmerged` | 三层都不通过，附 `unmergedCommits` |
| | `targetMissing` | 仓库无 `refs/remotes/origin/<target>` |
| | `branchMissing` | `branch` 模式且 live 分支已不存在 |
| | `notCheckable` | worktree 目录缺失 / 未注册 / 源仓库缺失 |
| | `error` | Git 非预期退出码，`errorMessage` 非空 |
| 附加标志 | `stale` | 本次 fetch 失败，结果基于本地缓存 |
| | `dirty` | worktree 有未提交改动 |
| `MergeCheckPhase` | `fetching \| checking \| record` | 见 003 |
| `Lifecycle` | 仅 `active` 参与 | 见 007 |

---

## 5. 测试要点（对应 `implementation-plan.md` S5）

fixture 用 `tempfile` + 本地裸仓库作 `origin`，本机无 `git` 时打印原因并返回。

1. 真 merge：feature 合入 develop → 层 1 `merged`。
2. rebase 后合并：feature rebase 到 develop 再 ff 合入；本地 feature 保留旧 commit → 层 2 `merged`。
3. squash 合并：`merge --squash` 后 commit → 层 3 `contained`；断言 `unmergedCommits` 为空。
4. 未合并：新增 commit 未推 → `unmerged` 且 `unmergedCommits` 长度等于新增 commit 数，元素以 7 位 hash 开头。
5. 远端只有 `master` 无 `develop` → `develop` 格 `targetMissing`，`master` 正常判定。
6. fetch 失败（把 origin URL 改为不存在路径）→ 两格 `stale = true` 且状态仍按本地缓存给出。
7. detached 记录：HEAD commit 已在 master 上 → `merged`。
8. 本地分支已删（`branch -D`）→ `branchMissing`。
9. worktree 目录被手工删除 → `notCheckable`。
10. `parse_cherry` 单测：混合 `+`/`-` 行、空输出、末尾无换行。
11. 层 3 冲突（退出码 1）→ `unmerged` 不是 `error`。
12. 操作锁：持锁期间调用 `check_merge_status` 得 `busy`。
13. 过滤参数：传 `worktreePath` 只返回一条。

---

## 6. 已知局限

- **squash 后又在 feature 上改同一批文件** → 层 3 结果树可能仍等于目标树，误报 `contained`。
- **squash 合并时冲突解决改写了内容** → 层 3 结果树与目标树不同，可能误报 `unmerged`。
- **合并后被 revert** → 层 1 仍判 `merged`（祖先关系不变），显示「已合并」但实际内容已撤销；反之 revert 后再判层 3 会显示「未完全包含」。
- **对比对象是分支 tip**，未提交改动不在任何分支中，只由 `dirty` 标志提示。
- **本地分支已删的记录无法判断**（`branchMissing`），不会尝试用 `baseCommit` 或 reflog 恢复。
- 层 3 依赖 `git merge-tree --write-tree`（Git ≥ 2.38）；更旧版本退出码非 0/1 → 该格 `error`。
- `stale` 时结果基于上次 fetch 的远端缓存，可能任意陈旧。
- 不区分「未合并」与「合并进了其他分支（如 release）」。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

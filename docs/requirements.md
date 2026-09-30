# Worktree Manager · 需求与判定口径

> 本文回答「做什么、不做什么、边界条件怎么判」。判定口径 13 条是**写死在实现里**的规则，与直觉冲突时以本文为准。

---

## 1. 使用者与场景

一个开发者在一个迭代里要同时改动 10+ 个仓库（PHP / Go 混合）。他要把这些仓库的 worktree 统一落到 `{工作区根目录}/{迭代号}/{项目}` 下，把公共目录（如 `fd-common`）复制一份供所有项目共用，把 PHP 的 `vendor` 按 lock 一致性搬过来，然后在一个界面里复核每个 worktree 的真实状态、检查改动是否已合并进 `develop` / `master`，并在确认安全的前提下移除单个 worktree 或归档整个迭代。

### 术语表

| 术语 | 含义 |
| --- | --- |
| 迭代 / iteration | 批次名，如 `7.3.0`、`sprint-42`；是工作区根目录下的单层目录名 |
| 工作区根目录 | 存放所有迭代目录的父目录；**无默认值，用户必填** |
| 公共目录规则 | 「源目录 → 迭代目标目录名」映射；**无默认规则** |
| 托管记录 | 迭代清单 `.worktree-manager.json` 里记录的 worktree |
| discovered worktree | 磁盘上存在、但清单没有记录的 worktree |
| 基分支 baseRef | 创建 worktree 时使用的基分支，归一化形如 `origin/release` |
| 基准变动 hasChanges | HEAD 相对**创建时记录的基分支**是否有本侧独有提交 |
| 未提交改动 dirty | `git status --porcelain` 非空 |
| 快扫 | 只枚举目录 + 读清单，**不跑任何 Git** |
| 复核 | `git worktree list` + HEAD/分支 + dirty + discovered 扫描 |
| live 名 | worktree 当前真实检出的分支名 |

---

## 2. 能力范围

### 2.1 MVP 能力（11 条，全部必须实现）

1. 配置 worktree 根目录、公共目录「源目录 → 目标目录」映射、人工选择的源项目。
2. 从本次 `git fetch <remote> <branch>` 解析出的不可变 Commit 串行创建多个 worktree。
3. 支持 Detached HEAD 或基于指定基分支创建全新本地分支。
4. 统一分支名与统一基分支同已选项目**单向**实时联动，逐项目仍可单独修改。
5. 按设置中的映射向迭代目录复制不含 `.git`、不跟随链接的公共目录快照；任何目录名都只是配置，无内建逻辑。
6. 添加项目时识别 PHP / Go / 其他项目，并显示 PHP 源仓库是否具备 `vendor`。
7. 创建时实时复核 PHP 项目，在两份 `composer.lock` 内容 Hash 相同时从源仓库复制 `vendor`。
8. 通过迭代清单复核并展示工作区真实状态（快扫 / 复核两档）。
9. 用系统文件管理器打开项目目录或迭代目录。
10. 在没有修改、未跟踪文件、独立提交或 worktree lock 风险时安全移除单个 worktree。
11. 全局配置与迭代清单均原子写入，不保存任何 Git 凭证。

### 2.2 增强能力（14 项，全部必须实现）

| 编号 | 能力 | 一句话要求 | 设计文档 |
| --- | --- | --- | --- |
| 001 | 迭代/分支合并状态检查 | 三层自动判定（merge / cherry / merge-tree）每条记录 × `origin/develop`、`origin/master` 的合并矩阵 | `design/001-merge-check` |
| 002 | 复制路径与分支 | 单击「Worktree 路径」「分支 / HEAD」两格复制到剪贴板，不新增后端命令 | `design/002-copy-path-and-branch` |
| 003 | 增量合并检查 | 一次命令 + 事件流，逐条完成即推送，前端逐行就地更新 | `design/003-incremental-merge-check` |
| 004 | 手动复核 | 列表分「快扫 / 复核」两档，复核只由「复核状态」按钮触发 | `design/004-manual-reconcile` |
| 005 | 相对基分支的「有变更」 | 看 HEAD 相对记录创建时的基分支，不看 `baseCommit` | `design/005-changes-relative-to-base-ref` |
| 006 | 合并检查结果并入「基准变动」 | 复用逐条事件，不另跑复核 | `design/006-merge-result-into-list` |
| 007 | 合并检查只查 active | 与列表可见行一致 | `design/007-merge-check-active-only` |
| 008 | 迭代归档 | 评估 + 批量移除 + 写 `archivedAt`；干净记录不带 `--force` | `design/008-archive-iteration` |
| 009 | 迭代备注 | 清单 `note`，≤50 字符、单行，空串＝清除 | `design/009-iteration-note` |
| 010 | 迭代隐藏 | 写 `hiddenAt`，移入页面底部「已隐藏迭代」收纳区，可恢复 | `design/010-iteration-hidden` |
| 011 | 分支改名同步 | 清单分支本地已不存在、worktree 仍注册同路径且检出别的分支 → 判「本地改名」，按 live 名给状态并原子回写清单 | `design/011-branch-rename-sync` |
| 012 | 备注气泡 | 悬浮显示全文，`Teleport` 到 body | `design/012-note-tooltip` |
| 013 | 创建基分支 | 创建页可选统一/逐项目基分支，`<datalist>` + 手输；远端列举本地优先、按需拉取 | `design/013-create-base-ref` |
| 014 | 项目拖拽排序与跨迭代移动 | 迭代内拖拽重排 `projects` 数组；跨迭代用 `git worktree move` 真实搬运并迁移清单记录 | `design/014-drag-reorder-and-move` |

### 2.3 明确不做

整迭代删除（008 归档不是删除，归档保留迭代目录与公共目录）、公共目录刷新/重新复制、自动扫描磁盘上的 Git 仓库、Git 凭证管理、`git pull`、`composer install`、打开 IDE/终端、分支自动删除、强制移除（008 的强制归档例外）、任务取消、定时刷新、自动更新、登录、云同步、团队功能。详见 `todo.md`。

---

## 3. 判定口径（13 条，写死在实现里）

1. **创建串行、失败隔离**：多项目按 UI 顺序串行；单项目失败不影响后续项目，也不回滚已成功项目；每个项目处理完立即持久化清单。**「已存在」判定**（`alreadyExists`，去重键＝`projectId` + `branch`，不含基分支）：清单中已有同键的 `active` 记录，且该记录目录存在并出现在源仓库 `worktree list --porcelain` 中（创建流程对每个源仓库执行一次该命令并缓存）；记录存在但无效时，按新记录处理、目录名顺延。detached 请求不参与去重（`branch = null` 永远新建）。
2. **fetch → 不可变 Commit**：每项目执行一次 `fetch <remote> <branch>`，立即把本次 `FETCH_HEAD` 解析为不可变 Commit Hash，Detached 与新分支都直接从该 Hash 创建；清单同时写归一化 `baseRef`。**不使用**「最新 master」这类会漂移的引用。
3. **公共目录先行**：在任何 Git 操作前按配置顺序完成全部公共目录；任一失败 → 整批终止、清单记 `failed`、不开始 fetch。目标已存在且由清单管理（清单 `sharedDirectories` 中有同 `ruleId` 且 `status ∈ {copied, reused}`）则复用（不覆盖）；未被清单管理的同名目录或活动 worktree 阻止创建。复制排除任意层级 `.git`，不跟随符号链接/Junction，先复制到同父目录临时目录再原子 rename 落位。没有配置规则时此步为空操作。
4. **vendor 六条件**：① 新 worktree 有 `composer.json`；② 源仓库有 `vendor` 目录；③ 新 worktree 有 `composer.lock`；④ 源仓库有 `composer.lock`；⑤ 两份 lock 的 SHA-256 完全一致；⑥ 目标没有 `vendor`。不满足是可解释的**跳过**而非创建失败；复制失败保留 worktree 并写清单。**不执行 `composer install`**。
5. **快扫 vs 复核**：启动、保存设置、创建/移除/归档/排序/移动后只做快扫（不跑 Git，存在的 worktree 标 `unknown`＝「未复核」）；完整复核只由「复核状态」按钮触发，复核中保留旧列表、完成后整体替换。不提供刷新按钮与定时轮询。
6. **基准变动**：`hasChanges` ＝ HEAD 相对「创建时记录的基分支」有本侧独有提交（`rev-list --count <base>..HEAD > 0`）。解析链：`baseRef` → 去掉 remote 前缀的本地名 → `origin/master` → `master`，取第一个可解析者；都不可解析 → `null`。discovered 行无清单记录，直接用 `origin/master`（否则 `master`）。`dirty` ＝ `status --porcelain` 非空。列表优先用合并检查给出的 `hasChanges`/`dirty`。
7. **合并三层判定**（前者命中即止）：① `merge-base --is-ancestor <branch> origin/<target>` → `merged`；② `git cherry origin/<target> <branch>` 输出无 `+` 行 → `merged`；③ `merge-tree --write-tree origin/<target> <branch>` 成功且结果树 == `origin/<target>^{tree}` → `contained`（疑似 squash）；都不通过 → `unmerged` 并附 `cherry` 的 `+` 行对应 commit 列表。目标分支固定 `origin/develop` + `origin/master`，不配置化；仓库缺该分支 → 该格 `targetMissing`，不整体失败。单仓库 fetch 失败 → 降级用本地 remote-tracking 缓存并给该仓库全部结果标 `stale`。对比基于 fetch 后的远端跟踪引用，不用本地 `develop`/`master`。**只查 `lifecycle=active`**。`branch` 模式用 live 分支名；`detached` 用 HEAD commit；分支已被删 → `branchMissing`；worktree 目录缺失 → `notCheckable`。
8. **移除风险**：实时检查 tracked 修改（`trackedChanges`）、untracked 文件（`untrackedFiles`）、未被任何 `refs/remotes` 包含的提交（`unpushedCommits`）、Detached 状态下 HEAD 不被任何 remote 引用包含（`detachedCommits`）、worktree `locked`（`worktreeLocked`）、路径越界或未注册（`pathInvalid`）、清单损坏（`manifestInvalid`）、`prunable`（`prunable`）；任一风险默认阻止，**不提供 `--force`、不 stash、不 commit、不 push、不删分支**。唯一 untracked 内容是**工具复制的 vendor**（清单 `vendor.copiedByTool = true` 且 untracked 路径全部以 `vendor/` 开头）时，`vendorOnlyCleanupAvailable = true`，可在输入 `{迭代号}/{目录名}` 二次确认后先删 vendor 再执行不带 `--force` 的 `worktree remove`。执行前重新计算风险并校验确认文本严格相等。成功后保留本地分支、迭代目录、所有公共目录与清单历史（记录改 `removed` + `removedAt`）。
9. **归档 clean 判定**：develop 与 master 单元格均为 `merged | contained | targetMissing`、`dirty = false`、本次 fetch 未失败（`stale = false`）。`clean` 与 `blockers` 由后端判定，前端不推导。归档**全部成功才写 `archivedAt`**；单条失败不中断整批；非 `clean` 且 `force = false` → 整体拒绝不动任何记录；`force = true` 时不干净的记录带 `--force`，干净的仍不带。
10. **优先级**：`archivedAt` 非空 → 迭代**完全不出现**在列表里（`list_groups` 直接跳过，快扫与复核一致）；`hiddenAt` 非空 → 仍投影，但由前端移入页面底部「已隐藏迭代」收纳区。归档保留本地分支、迭代目录与公共目录。
11. **本地改名判定**：清单里的分支名在源仓库已不存在、而 worktree 仍注册在同一路径并检出了另一个分支 → `validity = valid`、`branchDisplay` 取 live 名、`renamedFrom` 记下清单旧名，并把清单 `branch` **原子回写**为 live 名（只改这一字段）；清单分支仍存在但与 live 不一致（切分支/串台）仍判 `headMismatch` 且清单不动。合并检查与安全移除同样按 live 名判定。
12. **基分支归一化**：空串＝`origin/master`；`<remote>/<branch>` 首段与已配置 remote 同名时按该 remote 解析；裸分支名优先 `origin`，没有 `origin` 且只有一个 remote 时用该 remote；remote 不存在或分支名非法 → 该项目失败。`createFailed` 记录同样保留归一化结果。
13. **排序与跨迭代移动**：落点语义统一为「插到锚点行之前，锚点为 `null` 即追加到该迭代末尾」，前端只提交锚点、不做索引换算；同迭代 drop 走 `reorder_project`，跨迭代走 `move_project`。目标目录名沿用 `projectId`/`projectId-N`，**被目标迭代既有记录占用就顺延——包括 `removed`/`createFailed` 这类仍留在清单里的非活动记录**。移动先 `git worktree move` 搬运本体，成功后再写两份清单（先目标、后源，各自原子写）。

---

## 4. 非功能约束

- **平台**：macOS 优先验收；代码跨平台（见 `architecture.md §2`）。
- **安全**：见 `architecture.md §4`；前端不可信、路径逐段 join、Git 参数向量、CSP 无远程资源。
- **并发**：全局单操作锁，非阻塞，不取消（`architecture.md §6.2`）。
- **持久化**：原子写；损坏清单只报错。
- **依赖**：白名单（`architecture.md §1`）。
- **语言**：中文文案与文档；标识符、Git 术语英文。
- **性能**：快扫对 20 个迭代 × 15 个项目应在 200ms 内返回（只读文件）；复核与合并检查时间由 Git 决定，UI 不阻塞（命令为 `async`，Git 子进程在 `tauri::async_runtime::spawn_blocking` 中执行）。

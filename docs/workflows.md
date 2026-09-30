# Worktree Manager · 流程说明

> 本文是系统**当前态的单一事实来源**之一（另两份是 `architecture.md` 与 `data-model.md`）。它按「用户动作 → 前端调用 → 后端模块 → Git 调用 → 写盘 → 事件 → 失败处理 → 完成后刷新档位」描述每条端到端流程。
> 更新规则：功能合入时同步更新对应流程；新增命令、新增 Git 子命令、新增写盘点必须先改本文与 `architecture.md §5/§7`，再改代码。Git 调用一律引用 `architecture.md §5` 的参数向量原文。
> 约定：「快扫」＝`listWorkspaces(false)`；「复核」＝`listWorkspaces(true)`。锁名见 `architecture.md §6.2`。

---

## 1. 首次启动与设置

**入口**：应用启动；侧栏「设置」。

1. `App.vue` 挂载 → `getConfig()` → `get_config` → `config.rs::load`：
   - 文件 `platform::config_dir()/config.json` 不存在 → 返回默认配置（`workspaceRoot: null`、三个空数组），**不创建文件**。
   - 存在旧格式 → `config.rs::migrate_legacy` 仅在内存迁移（`fdCommonSource` → 一条 `fd-common` 规则；`goCommonSource` 与 `enabled=false` 规则丢弃）。
   - JSON 损坏 → 返回 `io` 错误，前端顶部横幅显示 `message`，页面仍可进入设置页重新填写并保存（保存会覆盖损坏文件）。
2. 随后执行一次快扫（见流程 3）；`workspaceRoot` 为 `null` 时后端直接返回空数组，列表页显示「请先在设置中填写工作区根目录」空态。
3. 浏览器演示模式（无 `__TAURI_INTERNALS__`）：`api/tauri.ts` 返回 `demo-data.ts` 假数据，顶部横幅「浏览器演示模式：数据为只读假数据」。
4. 设置页「选择目录」：动态 `import("@tauri-apps/plugin-dialog")` → `open({ directory: true })`，回填 `workspaceRoot` 输入框。**不触发任何后端命令**。
5. 设置页「添加项目」：`open({ directory: true, multiple: true })` → `resolveProjects(paths)` → `resolve_projects` → `config.rs::resolve_projects`，逐路径：
   - `git rev-parse --show-toplevel`（工作目录＝该路径）→ 失败则该条 `error: "不是 Git 仓库"`，其余字段为 `null`。
   - `canonicalize` 仓库根 → `detect_project_type`（`composer.json` → `php`；`go.mod` → `go`；否则 `other`）→ `vendorAvailable`（`php` 且 `vendor/` 是目录）→ `suggestedId` ＝ 根目录名（若不通过 `validate_project_id` 则为 `null`，由用户手填）。
   - 前端逐条显示解析结果或错误，成功项追加到项目表（`id` 可编辑）。
6. 「保存」：`saveConfig(config)` → `save_config` → `config.rs::normalize_and_validate` + `save`：
   - `workspaceRoot` 去首尾空白，空串 → `null`；非空必须绝对路径，允许尚不存在（`path_utils::resolve_allow_missing` 校验最近已存在祖先）。
   - `sharedDirectories[].sourcePath` 必须存在且 `canonicalize`；`targetDirectory` 过 `validate_directory_name` 且大小写不敏感唯一。
   - `projects[].repositoryPath` 必须是已存在 Git 根并 `canonicalize`；`id` 过 `validate_project_id`（含与所有 `targetDirectory` 的冲突集合）。
   - `recentIterations` 去重、限 10 条。
   - **写盘**：`atomic_json::write_json_atomic(config.json)`（临时文件 → flush → sync_all → rename）。首次保存时创建 `WorktreeManager` 目录。
   - 返回规范化后的 `AppConfig`；前端用返回值回填表单，toast「设置已保存」。
7. 「从列表移除项目」只改表单，保存后只改 `config.json`，**不动磁盘仓库**，UI 文案明示。
8. **锁**：本流程所有命令不持锁。
9. **完成后**：保存成功 → 快扫。

---

## 2. 创建工作区

**入口**：侧栏「工作区」→「新建工作区」按钮 → 创建页 → 底部固定操作条「开始创建」。

前置（前端禁用按钮并显示原因，后端仍重新校验）：`workspaceRoot` 非空、迭代号通过前端格式预检、至少勾选一个项目。

1. 创建页挂载：从 `config` 读项目与 `recentIterations`（胶囊）；**不发起任何远端连接**（基分支候选见流程 13 说明，位于 `design/013`）。
2. 统一分支名 / 统一基分支输入 → 覆盖所有已勾选行；新勾选行继承当前统一值；单行修改不反向影响统一值（纯前端状态）。
3. 「开始创建」→ `createWorkspaces(request)` → `create_workspaces`：
   - 获取锁「创建工作区」；已占用 → `busy`，toast「已有操作执行中：<名>」。
   - 订阅 `onCreateProgress` 后再发起 `invoke`（避免漏掉首个事件）；页面切到 `OperationView`。
4. `workspace.rs::create_batch(config, request, emit)`：
   1. `validate_iteration(request.iteration)`；`workspaceRoot` 为 `null` → `validation`「请先在设置中填写工作区根目录」。
   2. `path_utils::resolve_allow_missing(workspaceRoot)`；不存在则 `create_dir_all`，再 `canonicalize`；迭代目录 ＝ `join_segments(root, iteration)`，不存在则创建。
   3. 读清单健康度：`missing` → 新建 `Manifest { createdAt: now, note: null, hiddenAt: null, archivedAt: null }`；`valid` → 复用；`damaged` → `manifestDamaged` 错误，**不写盘**；`archivedAt` 非空 → `conflict`「该迭代已归档」。
   4. `request.note` 有值 → 走流程 10 的校验后写入；缺省 → 不改。`hiddenAt` 非空 → 清为 `null`（已隐藏迭代上继续创建自动取消隐藏）。
   5. **写盘**：清单初始化后立即原子写一次。
   6. `config.recentIterations` 前插本迭代号并去重限 10 → **写盘** `config.json`（本批次仅一次）。
   7. **公共目录先行**（判定口径 3），按配置顺序逐条：
      - 目标 ＝ `join_segments(迭代目录, targetDirectory)`。
      - 清单已有同 `ruleId` 且 `status ∈ {copied, reused}` 且目录存在 → 状态 `reused`。
      - 目录存在但清单未管理（或该路径在任一已配置仓库的 `git worktree list --porcelain` 中注册）→ 该条 `failed`，`message`「目标已存在且不受清单管理」。
      - 否则 `copy.rs::copy_snapshot(sourcePath, target)`：排除任意层级 `.git`、遇链接（`platform::is_link_like`）报错、先复制到同父目录 `.tmp-*` 再 `rename`；成功 → `copied`。
      - 任一条 `failed` → **写盘**清单（记录该条 `failed`）→ `aborted = true`、`abortReason` → 直接返回，**不开始任何 fetch**。
      - 没有配置规则 → 本步空操作。
   8. 逐项目串行（判定口径 1、2、4），每项目：
      1. `emit(create-progress, phase=queued)`。
      2. `base_ref.rs::normalize`：`git remote` 列举 → 归一化（判定口径 12）；失败 → 记录 `createFailed`（保留归一化尝试结果与 `message`），`emit(failed)`，继续下一项目。
      3. 分支模式：`validate_branch_name` + `git check-ref-format --branch <name>`；`git show-ref --verify --quiet refs/heads/<branch>` 退出码 0 → 本地已有同名分支 → `createFailed`，**不建目录**。
      4. 去重（键＝`projectId` + `branch`，不含基分支）：清单已有 `active` 记录且目录存在 → `createResult.status = alreadyExists`，不再 fetch，`emit(completed)`，继续。
      5. 目录名：`path_utils::next_directory_name(occupied, projectId)`，`occupied` ＝ 清单所有记录（含 `removed`/`createFailed`）的目录名 ∪ 公共目录目标名 ∪ 迭代目录现有子目录名。
      6. 目标目录已存在且不由清单管理 → `createFailed`「目录已存在且不受清单管理」。
      7. `emit(fetching, message="git fetch <remote> <branch>")` → `git fetch <remote> <branch>`（工作目录＝源仓库根）；失败 → `createFailed`（`message` 已脱敏截断）。
      8. `git rev-parse --verify FETCH_HEAD^{commit}` → `baseCommit`（40 位 hex）。
      9. `emit(creating, message="git worktree add …")`：
         - detached：`git worktree add --detach <path> <commit>`
         - 分支：`git worktree add -b <branch> <path> <commit>`
         - 失败 → `createFailed`，若目录被部分创建不做清理（记录 `message`，由复核暴露）。
      10. `emit(vendor)` → `vendor.rs::assess_vendor(worktree, sourceRepository)` 六条件 → 满足则 `copy_vendor`（同 `copy_snapshot` 的临时目录 + rename 机制），`vendor.copiedByTool = true`；不满足 → 对应 `VendorStatus` 跳过；复制失败 → `copyFailed` + `postSteps[vendor].status = failed`，worktree 保留。非 PHP → `notPhp`。
      11. 记录写入清单 `projects[]` 末尾（`lifecycle: active`、`createResult.status: created`、`createdAt: now`、`baseRef` 归一化值）→ **写盘**（每项目一次原子写）。
      12. `emit(completed | failed, message)`。
   9. 返回 `CreateBatchResult`。
5. **事件**：`create-progress` 载荷见 `data-model.md §4`；`message` 必须含实际命令文本。
6. **失败处理**：单项目失败不影响后续、不回滚已成功项目；公共目录失败整批终止；锁冲突不进入 `OperationView`。`OperationView` 每行显示阶段标签与 `message`，全部结束显示「返回列表」。
7. **完成后**：前端 `unlisten()`，回到列表页 → 快扫。

---

## 3. 启动列表与手动复核

**入口**：应用启动 / 返回列表页 / 保存设置后（快扫）；页面标题右侧「复核状态」按钮（复核）。

### 3.1 快扫 `listWorkspaces(false)` → `list_workspaces(reconcile=false)` → `manifest.rs::list_groups(config, false)`

1. `workspaceRoot` 为 `null` 或目录不存在 → 返回 `[]`。
2. 枚举根目录一级子目录（不递归，跳过以 `.` 开头的目录），按名称排序。
3. 每个子目录：`manifest.rs::health_of(dir)` → `missing | damaged | valid`。
   - `damaged` 的判定：JSON 解析失败、`schemaVersion ≠ 1`、`iteration` 与目录名不一致、`worktreePath` 规范化后重复。
   - `archivedAt` 非空 → **跳过该迭代**（不进入返回数组）。
4. `valid` 时投影每条记录（`manifest.rs::project_snapshot`）：`removed` → `removed`；目录不存在 → `missingDirectory`；目录存在 → `unknown`；`createFailed` 且目录不存在 → `missingDirectory`。`dirty`/`hasChanges` 为 `null`；`openable` ＝ 目录存在；`removable` 按 `data-model.md §5`。
5. `missing`/`damaged`：`projects: []`，`manifestMessage` 说明原因，`openable` ＝ 目录存在。
6. **不跑任何 Git，不写盘，不持锁。**
7. 前端整体替换 `groups`，按 `hiddenAt` 拆成主列表与「已隐藏迭代」收纳区。

### 3.2 复核 `listWorkspaces(true)` → `list_workspaces(reconcile=true)` → `manifest.rs::list_groups(config, true)`

1. 获取锁「复核状态」；已占用 → `busy`。前端按钮进入 loading，**旧列表保留**。
2. 先执行 3.1 的枚举与清单读取。
3. 对每个 `valid` 迭代的每条 `active` 记录：
   1. 源仓库不存在或不是 Git 根 → `sourceMissing`。
   2. 按源仓库缓存执行一次 `git worktree list --porcelain`（`git.rs::parse_worktree_list`，完整解析），查找规范化路径等于 `worktreePath` 的条目；目录不存在 → `missingDirectory`；目录存在但无条目 → `notRegistered`。
   3. `git rev-parse --verify HEAD` 与 `git symbolic-ref -q --short HEAD`（工作目录＝worktree）→ live HEAD 与 live 分支名（detached 时 `symbolic-ref` 非零退出）。
   4. `head_state.rs::judge(record, entry, source_repo)`：需要时执行 `git show-ref --verify --quiet refs/heads/<清单branch>`（工作目录＝源仓库）：
      - 清单分支不存在 + 同路径注册 + live 分支存在且不同 → `Renamed { live }` → `validity = valid`、`branchDisplay = live`、`renamedFrom = 旧名`，并将清单 `branch` 改为 live 名，计数 +1。
      - 清单分支仍存在但 live 不同 → `headMismatch`，清单不动。
      - 一致（或 detached 且 HEAD == `baseCommit` 或任意 commit）→ `valid`。
   5. `git status --porcelain=v1 --untracked-files=all` → `dirty` ＝ 输出非空。
   6. `hasChanges`（判定口径 6）：解析链 `baseRef` → 去 remote 前缀本地名 → `origin/master` → `master`，每个候选用 `git rev-parse --verify <ref>^{commit}` 试探，首个成功者作 `<base>`；`git rev-list --count <base>..HEAD > 0` → `true`；无候选可解析 → `null`。
   7. `removed` 记录不跑 Git，直接 `removed`。
4. **discovered 扫描**：对每个已配置项目的源仓库，取上一步缓存的 `worktree list --porcelain`，其中路径位于该迭代目录之下、且不等于任何清单记录的 `worktreePath` 的条目 → 追加一条 `validity = discovered`、`lifecycle = null`、`sourceRepository = 该仓库`、`branchDisplay = live 名或 detached @ <短hash>` 的行；同样计算 `dirty`；`hasChanges` 用 `origin/master`（否则 `master`）。迭代目录下含 `.git` 文件但未匹配到任何已配置仓库的子目录 → 也列为 `discovered`，`sourceRepository` 为空串、`removable = false`。
5. **写盘**：仅当存在 `Renamed` 时，对该迭代清单执行一次 `atomic_json::write_json_atomic`（只有 `branch` 字段值变化）。
6. 返回后前端**整体替换**列表；`renamed` 计数 N > 0 时 toast「已同步 N 个分支重命名」。
7. **失败处理**：单条记录的 Git 失败 → 该行 `validity = unknown`、`manifestMessage` 不变，其他行继续；整体 I/O 错误 → 顶部横幅，旧列表保留。
8. 无刷新按钮、无定时轮询。

---

## 4. 打开目录

**入口**：迭代卡片头「打开目录」；项目行「打开」图标按钮。按钮在 `openable = false` 时禁用。

1. `openIteration(iteration)` → `open_iteration`：`validate_iteration` → `join_segments(root, iteration)` → 必须存在并 `canonicalize` → `path_utils::ensure_within(root, path)`。
2. `openProject(iteration, projectId, worktreePath)` → `open_project`：读清单找到该记录（或 discovered 行时按 `worktreePath`）→ `canonicalize` → `ensure_within(迭代目录, path)`；不存在 → `notFound`。
3. `platform::open_in_file_manager(path)`：macOS `open <path>`；Windows `explorer <path>`；Linux `xdg-open <path>`（参数向量，不拼接 shell）。
4. **不持锁、不写盘、无 Git 调用。**
5. 失败 → toast（danger）显示 `message`。

---

## 5. 复制路径与分支

**入口**：项目表格「Worktree 路径」「分支 / HEAD」两格单击（`cursor: copy`）。

1. 纯前端：`utils/clipboard.ts::copyText(text)` → `navigator.clipboard.writeText`；失败（权限/非安全上下文）→ 降级为隐藏 `textarea` + `document.execCommand("copy")`。
2. 复制内容：路径格＝`worktreePath` 全文；分支格＝`branchDisplay`（detached 行为 `detached @ <短hash>` 时复制完整 HEAD hash）。
3. 成功后该格显示「已复制」1.2 秒后恢复；失败 toast「复制失败」。
4. **不新增后端命令、不占锁、不写盘。**拖拽手柄独立于此，单击格子不会触发拖拽。

---

## 6. 合并状态检查

**入口**：迭代卡片头「检查合并」（`manifestHealth ≠ valid` 时禁用）。

1. 前端先订阅 `onMergeCheckProgress`，再 `checkMergeStatus(iteration)` → `check_merge_status`。
2. 获取锁「检查合并」；已占用 → `busy`。
3. `merge_check.rs::check(config, iteration, filter, emit)`：
   1. 读清单；`damaged` → `manifestDamaged` 错误。
   2. 只取 `lifecycle = active` 的记录（判定口径 7；`projectId`/`worktreePath` 过滤器可选）。按源仓库分组。
   3. 每个源仓库一次：`emit(fetching, message="git fetch --no-tags origin develop master")` → `git fetch --no-tags origin develop master`；失败 → 该仓库 `stale = true`，改用本地 `refs/remotes/origin/*` 缓存，不中断。
   4. `git for-each-ref --format=%(refname) refs/remotes`（每仓库一次）→ 判定 `refs/remotes/origin/develop` / `refs/remotes/origin/master` 是否存在；不存在 → 该格 `targetMissing`。
   5. 每条记录 `emit(checking, index, total)`：
      - 通过 `git worktree list --porcelain` + `head_state.rs::judge` 取 live 名（改名记录按 live 名判定，见流程 3.2；此处**不回写清单**）。
      - 目录不存在 → 两格 `notCheckable`；分支模式且 `git show-ref --verify --quiet refs/heads/<live>` 失败 → `branchMissing`；detached 用 HEAD commit 作 `<head>`。
      - 每个目标 `<t> ∈ {develop, master}` 三层判定：
        1. `git merge-base --is-ancestor <head> origin/<t>` 退出 0 → `merged`。
        2. `git cherry origin/<t> <head>` 输出无 `+` 开头行 → `merged`。
        3. `git merge-tree --write-tree origin/<t> <head>` 成功，且结果树 == `git rev-parse origin/<t>^{tree}` → `contained`。
        4. 否则 `unmerged`，`unmergedCommits` ＝ `cherry` 中 `+` 行对应的 commit（短 hash + subject，见 `design/001`）。
        5. 任一 Git 命令异常退出（非 1/0 的判定退出码）→ `error` + 脱敏 `errorMessage`。
      - `dirty` ＝ `git status --porcelain=v1 --untracked-files=all` 非空；`hasChanges` 同流程 3.2 第 6 步（判定口径 6）。
      - 组装 `MergeRecordResult` → `emit(record, record=该条)`。
   6. 返回 `MergeCheckResult { checkedAt: now, records }`。
4. **写盘**：无。结果只存前端内存 `Map<iteration, MergeCheckResult>`。
5. **前端**：`phase=record` 时按 `worktreePath` 就地更新对应行的 `develop`/`master` 格与「基准变动」格（`hasChanges`/`dirty` 覆盖复核值）；命令返回后用完整结果再覆盖一次；组件卸载 `unlisten()`。点击合并格打开 `MergeDetailDialog` 显示 `unmergedCommits`/`errorMessage`/`stale`。
6. **失败处理**：单仓库 fetch 失败 → `stale` 标「可能过时」并继续；整体错误 → toast，已推送的行保留。
7. **完成后**：不触发列表刷新（内存合并）。归档、移除、移动、创建成功后前端清除相关迭代的结果。

---

## 7. 安全移除（含 discovered）

**入口**：项目行「移除」图标按钮（`removable = false` 时禁用）。

### 7.1 评估

1. 托管行：`assessRemoval(iteration, projectId, worktreePath)` → `assess_removal`；discovered 行：`assessDiscoveredRemoval(sourcePath)` → `assess_discovered_removal`（`sourcePath` ＝ 该行 `sourceRepository`；为空串的 discovered 行不可移除）。**不持锁**。
2. `removal.rs::assess`：
   1. 托管：读清单，`damaged` → 风险 `manifestInvalid`（blocking）并返回；找记录，`lifecycle ≠ active` → `pathInvalid`。
   2. `canonicalize(worktreePath)` + `ensure_within(迭代目录)`；失败 → `pathInvalid`（blocking）。
   3. `git worktree list --porcelain`（源仓库）→ 未注册 → `pathInvalid`；`locked` → `worktreeLocked`；`prunable` → `prunable`。
   4. `head_state.rs::judge` 取 live 名（改名按 live 名）。
   5. `git status --porcelain=v1 --untracked-files=all` 完整解析：tracked 改动路径 → `trackedChanges`（`paths[]`）；untracked 路径 → `untrackedFiles`。
   6. `vendorOnlyCleanupAvailable` ＝ 无 tracked 改动 且 untracked 全部以 `vendor/` 开头 且 清单 `vendor.copiedByTool = true`。discovered 无清单 → 恒 `false`。
   7. 分支模式：`git for-each-ref --format=%(refname) --contains HEAD refs/remotes` 输出为空 → `unpushedCommits`；detached：同命令为空 → `detachedCommits`。
   8. 所有风险 `severity = blocking`；`allowed` ＝ 风险为空 或 唯一风险是 `untrackedFiles` 且 `vendorOnlyCleanupAvailable`。
   9. `confirmationText = "{iteration}/{目录名}"`。
3. `RemovalDialog` 展示风险列表（code 文案 + paths）、vendor 清理复选框（仅 `vendorOnlyCleanupAvailable` 时出现）、确认文本输入；`allowed = false` 时确认按钮禁用并说明「请先处理上述风险」。
4. **不执行 force、stash、commit、push、分支删除**。

### 7.2 执行

1. 托管：`removeWorktree({ iteration, projectId, worktreePath, confirmation, removeCopiedVendor })` → `remove_worktree`；discovered：`removeDiscoveredWorktree(iteration, projectId, worktreePath, sourcePath, confirmation, removeCopiedVendor)`。
2. 获取锁「移除 worktree」。
3. `removal.rs::remove`：
   1. **重新执行 7.1 全部评估**（不信任前端）；`confirmation` 必须与 `confirmationText` 严格相等 → 否则 `validation`。
   2. `allowed = false` → `conflict`，附风险列表。
   3. `removeCopiedVendor = true` 且 `vendorOnlyCleanupAvailable` → 删除 `worktree/vendor`（`remove_dir_all`，先 `ensure_within`）；删除后再跑一次 `git status --porcelain=v1 --untracked-files=all` 确认为空。
   4. `git worktree remove <path>`（**不带 `--force`**）。
   5. 托管：清单记录 `lifecycle = removed`、`removedAt = now` → **写盘**（原子）。discovered：无清单写入。
4. 成功后保留本地分支、迭代目录、所有公共目录与清单历史。
5. **失败处理**：Git 失败 → `git` 错误（脱敏），清单不动；toast 显示。
6. **完成后**：关闭对话框，清除该迭代的合并检查结果 → 快扫。

---

## 8. 归档迭代

**入口**：迭代卡片头「归档」（`manifestHealth ≠ valid` 时禁用）。

1. 前端订阅 `onArchiveProgress` → `assessArchive(iteration)` → `assess_archive`（持操作锁「评估归档」：它会对每个仓库 fetch，与检查合并互斥）。
2. `archive.rs::assess(config, iteration, emit)`：
   1. 复用 `merge_check.rs::check`，事件改发 `archive-progress`（`record` 恒 `null`，`phase` 沿用 `fetching|checking`）。
   2. 每条 `active` 记录：`clean` ＝ `develop, master ∈ {merged, contained, targetMissing}` 且 `dirty = false` 且 `stale = false`；`blockers[]` 由后端生成文案（如「develop 未合并」「有未提交改动」「fetch 失败，结果可能过时」）。
   3. `ArchiveAssessment.clean` ＝ 全部记录 `clean`；`confirmationText = iteration`；无 `active` 记录时 `clean = true`。
3. `ArchiveDialog` 两态：干净 → 显示记录表 + 确认输入；不干净 → 额外显示 `blockers` 与「强制归档（对不干净记录使用 --force）」复选框。
4. 「归档」→ `archiveIteration({ iteration, confirmation, force })` → `archive_iteration`。
5. 获取锁「归档迭代」。
6. `archive.rs::archive`：
   1. `confirmation == iteration` 严格相等，否则 `validation`。
   2. **重新执行评估**（不信任前端传来的状态）。
   3. `!clean && !force` → `conflict`「存在未满足归档条件的记录」，**不动任何记录**。
   4. 逐条 `active` 记录：该条 `clean` → `git worktree remove <path>`；该条不干净且 `force = true` → `git worktree remove --force <path>`。每条成功 → 记录 `lifecycle = removed`、`removedAt = now` → **写盘**（每条一次原子写）；失败 → 记入 `failed[]`，继续下一条。
   5. `failed` 为空 → 清单 `archivedAt = now` → **写盘**，`archived = true`；否则 `archived = false`。
   6. 返回 `ArchiveOutcome`。
7. 保留本地分支、迭代目录、公共目录、清单文件。
8. **失败处理**：单条失败不中断；结果对话框列出 `failed[]`，用户可修正后再次归档（已移除的记录不再处理）。
9. **完成后**：清除该迭代合并检查结果 → 快扫；`archivedAt` 非空的迭代不再出现在任何区域。

---

## 9. 隐藏与恢复迭代

**入口**：迭代卡片头「隐藏」；「已隐藏迭代」收纳区每项的「恢复」。

1. `setIterationHidden(iteration, true | false)` → `set_iteration_hidden`。
2. 获取锁「隐藏迭代」。
3. `manifest.rs`：读清单，`damaged` → `manifestDamaged`；`archivedAt` 非空 → `conflict`；`hidden = true` → `hiddenAt = now`；`false` → `hiddenAt = null` → **写盘**（原子）。返回新的 `hiddenAt`。
4. **无 Git 调用。**
5. 前端：用返回值更新该 `WorkspaceGroup.hiddenAt`，卡片在主列表与收纳区之间移动；收纳区默认折叠、无隐藏项整区不渲染、不受搜索影响、不渲染项目表。展开态存 `worktree-manager.hidden-zone-expanded`。
6. **失败处理**：toast。
7. **完成后**：本地更新即可，不强制快扫（下次快扫结果一致）。

---

## 10. 迭代备注

**入口**：迭代卡片头「✎ 备注」→ 内联单行输入（`maxlength=50`）→ 回车/「保存」；创建请求携带 `note`。

1. 前端 `utils/note.ts` 预检（长度、换行）仅用于即时提示，后端仍校验。
2. `setIterationNote(iteration, note)` → `set_iteration_note`（`note` 为 `null` 或空串均表示清除）。
3. 获取锁「修改备注」。
4. `manifest.rs`：读清单，`damaged` → `manifestDamaged`；`archivedAt` 非空 → `conflict`；`note` 去首尾空白 → 含 `\n`/`\r` → `validation`「备注不能换行」；Unicode 标量数 > 50 → `validation`「备注不能超过 50 字符」；空串 → `null` → **写盘**（原子）。返回规范化后的 `note`。
5. 创建流程（流程 2 第 4.4 步）复用同一校验；请求 `note` **缺省 ＝ 不改动**，前端不得用 `""` 表达「不改」。
6. **无 Git 调用。**
7. 前端：用返回值更新卡片；有备注显示 `▤`，hover 显示气泡（`NoteTooltip`，见 `design/012`）；无备注不渲染任何常驻痕迹。
8. **完成后**：本地更新，不强制快扫。

---

## 11. 调整顺序与跨迭代移动

**入口**：项目行手柄 `⠿` 拖拽（唯一拖拽起点）；落点为同卡片或其他卡片的行/表尾。`manifestHealth ≠ valid` 的迭代不接受落点；搜索过滤中手柄置灰不可拖。

### 11.1 同迭代排序

1. 前端计算锚点 `beforeWorktreePath`（目标行上半 → 该行；下半 → 下一行；表尾/空白 → `null`）；落点等于原位 → **不发请求**。
2. `reorderProject(iteration, worktreePath, beforeWorktreePath)` → `reorder_project`。
3. 获取锁「调整顺序」。
4. `relocate.rs::reorder`：读清单（`damaged` → 错误；`archivedAt` 非空 → `conflict`）→ 找到被移记录 → 从数组移除 → `before` 为 `null` 追加末尾，否则插到锚点记录之前（锚点不存在 → `notFound`）→ **写盘**（原子）。
5. **无 Git 调用。**
6. **完成后**：快扫（合并检查结果保留，因记录未变）。

### 11.2 跨迭代移动

1. `moveProject({ iteration, worktreePath, targetIteration, beforeWorktreePath })` → `move_project`。
2. 获取锁「移动项目」。
3. `relocate.rs::move_project` 预检（任一不通过即报错且**不改动任何内容**）：
   1. 两个迭代清单都 `valid`；目标 `archivedAt` 为 `null`。
   2. 源记录存在且 `lifecycle = active`。
   3. 源目录名匹配 `projectId` 或 `projectId-N`。
   4. `canonicalize(worktreePath)` 存在且 `ensure_within(源迭代目录)`。
   5. `git worktree list --porcelain`（源仓库）中注册同路径；`locked` → `conflict`「worktree 已锁定」；`prunable` → `conflict`「worktree 为 prunable，请先在源仓库处理」（**不得用 `-f -f` 绕过**）。
   6. 目标目录名 ＝ `next_directory_name(目标清单全部记录目录名 ∪ 目标公共目录名 ∪ 目标迭代现有子目录名, projectId)`（判定口径 13：`removed`/`createFailed` 也算占用）。
4. `git status --porcelain=v1 --untracked-files=all` 非空 → 使用单个 `-f`。
5. `git worktree move [-f] <旧路径> <新路径>`（工作目录＝源仓库）。失败 → `git` 错误，两份清单不动。
6. **写盘**（顺序固定）：
   1. 目标清单：在锚点前（`null` → 末尾）插入源记录副本，`worktreePath` 改为新路径，其余快照字段不变 → 原子写。
   2. 源清单：删除该记录 → 原子写。
7. 返回 `MoveOutcome`。
8. **失败处理**：第二次写入失败 → 错误信息附恢复指引「worktree 已搬到 <新路径>，目标清单已记录，源清单仍含旧记录；请点击「复核状态」（旧记录将显示目录缺失）或手工删除源清单中的该条记录」。不自动回滚。
9. **完成后**：前端清除源与目标两个迭代的合并检查结果 → 快扫。

---

## 12. 失败处理

### 12.1 清单缺失 / 损坏

- `list_groups` 对 `missing`/`damaged` 迭代返回 `projects: []` + `manifestMessage`；卡片显示 danger 横条。
- 该迭代「备注 / 隐藏 / 归档 / 检查合并」禁用，不能作为拖拽落点；「打开目录」按目录是否存在。
- 后端**只报错，不覆盖、不重建、不删除**损坏文件；用户手工修复后再快扫即可。
- 创建流程遇 `damaged` → 整批不开始；`missing` → 视为新迭代创建清单（目录已有内容不受影响，但同名目录冲突规则照常生效）。

### 12.2 操作锁冲突

- 任一持锁命令在锁被占用时**立即**返回 `{ code: "busy", message: "已有操作执行中：<当前操作名>" }`，不排队、不取消。
- 前端 toast（warning）显示原文；不重试。
- 持锁清单：创建、移除（两种）、评估归档、归档、复核、检查合并、备注、隐藏、排序、移动（操作名见 `architecture.md §6.2`）。`assess_removal`、`assess_discovered_removal`、`get_config`、`save_config`、`resolve_projects`、`list_remote_branches`、`open_*`、快扫不持锁。

### 12.3 Git 失败与脱敏

- `git.rs` 统一捕获非零退出：`stderr` 经 `error.rs::redact_credentials`（去掉 `scheme://user:token@host` 中的 `user:token@`、`scheme://token@host` 中的 `token@`）与 `truncate_message`（600 字符 + `…`）后作为 `AppError::Git.message`。
- 用于安全判定的输出（`worktree list --porcelain`、`status --porcelain`、`for-each-ref`）**完整读取不截断**；截断只发生在用户可见的错误文本上。
- 所有 Git 子进程 `GIT_TERMINAL_PROMPT=0`；需要凭证的操作在无凭证时直接失败并显示脱敏信息，不弹交互。

### 12.4 fetch 失败降级

- 创建（流程 2）：`fetch <remote> <branch>` 失败 → 该项目 `createFailed`，继续下一项目；不降级。
- 合并检查 / 归档评估（流程 6、8）：`fetch --no-tags origin develop master` 失败 → 该仓库全部结果 `stale = true`，用本地 `refs/remotes/origin/*` 继续；归档 `clean` 判定要求 `stale = false`，因此 fetch 失败的记录不可干净归档（仍可 `force`）。
- 远端分支列举（`list_remote_branches`，`design/013`）：`ls-remote` 15 秒超时或失败 → `source = local` 候选 + `warning`（脱敏原文），不阻断创建。

### 12.5 第二次写清单失败（跨迭代移动）

- 场景：`worktree move` 已成功、目标清单已写、源清单写入失败。
- 系统状态：磁盘与目标清单一致，源清单多一条指向旧路径的 `active` 记录。
- 处理：命令返回 `io` 错误并附恢复指引（流程 11.2 第 8 步）；不自动回滚、不引入跨文件事务；复核后该旧记录显示 `missingDirectory`，用户可手工删除源清单中的该条记录（或将其视为历史记录保留）。

### 12.6 其他

- `workspaceRoot` 为 `null`：快扫返回空；创建返回 `validation`；设置页标「必填」。
- 命令参数校验失败（迭代号、目录名、分支名、路径越界）→ `validation`，toast 显示；不写盘。
- 前端 `invoke` 抛出的非 `{code, message}` 对象（如 Tauri 运行时错误）→ 统一包装为 `{ code: "io", message: String(err) }`。

# Worktree Manager · 验收清单

> 规则：
> 1. **未真实验证不许打勾**。每条都必须由实现方亲自执行「验证方法」所述动作后才能改为 `- [x]`。
> 2. 无法验证的条目保持 `- [ ]`，并在条目后追加 `（未验证：<原因>）`。
> 3. 打勾时在条目后追加 `（验证于 S<步骤号>，<方式：单测 / 集成测试 / 真实窗口 / 文件检查 / 命令输出>）`。
> 4. 平台：本轮验收平台为 **macOS**。§11 的 Windows / Linux 条目默认标「未验证（非本轮平台）」。
> 5. 步骤对应关系见 `implementation-plan.md`：§1-§5 在 S4 验收，§6 在 S5/S6，§7 在 S6/S7，§8 在 S8，§9-§10 在 S9 全量复验。

---

## §1 配置

- [ ] 首次启动（配置文件不存在）设置页显示空的工作区根目录并带「必填」标记，公共目录规则表为空，项目表为空；不要求已有配置文件，且不会自动创建 `config.json`。
  验证方法：删除 `~/Library/Application Support/WorktreeManager/config.json` 后启动真实窗口，进入设置页观察；用 `ls` 确认文件未被创建。
- [ ] `workspaceRoot` 为空时列表页为空、创建页「开始创建」禁用并提示「请先在设置中填写工作区根目录」；`create_workspaces` 后端也返回 `validation` 错误。
  验证方法：真实窗口观察；`config.rs` / `workspace.rs` 单测覆盖 `workspaceRoot: null` 路径。
- [ ] 工作区根目录允许尚不存在：填写一个不存在但父目录存在的路径可以保存；父目录也不存在的祖先链会向上找到最近存在的祖先并通过校验。
  验证方法：`path_utils.rs` 单测 `resolve_allow_missing`；真实窗口填写 `~/tmp-not-exist/workspace` 保存成功。
- [ ] 可用系统目录多选对话框选择多个目录并解析到真实 Git 根目录（子目录也能解析到根）；普通目录给出明确错误「不是 Git 仓库」而不是崩溃。
  验证方法：真实窗口「添加项目」选一个仓库子目录与一个普通目录，观察两条解析结果；`config.rs::resolve_projects` 集成测试。
- [ ] 公共目录目标名称大小写不敏感唯一；项目标识大小写不敏感唯一；项目标识不能与任一已配置公共目录目标名大小写不敏感相等；项目标识与目标名都拒绝非法目录名。
  验证方法：`validation.rs` 单测（`Api3` 与 `api3` 冲突、`common` 与目标名 `Common` 冲突）；真实窗口保存时看到错误 toast。
- [ ] 保存后 `config.json` 是有效 UTF-8 JSON，`schemaVersion` 为 1，字段为 camelCase，且不含 remote URL / 用户名 / Token / 环境变量。
  验证方法：保存后 `cat ~/Library/Application\ Support/WorktreeManager/config.json | python3 -m json.tool`，`grep -i` 检查 `token` `password` `http` 无命中。
- [ ] 旧格式 `fdCommonSource` 迁移为一条目标为 `fd-common` 的规则；`goCommonSource` 与 `enabled=false` 的规则被丢弃；`enabled=true` 的规则保留并去掉 `enabled` 字段；迁移只在内存中进行，用户保存后才写回新格式。
  验证方法：`config.rs::migrate_legacy` 单测；手工写一份旧格式文件启动，设置页显示迁移结果，未保存前文件内容不变。
- [ ] `recentIterations` 最多 10 条、最近在前、去重、只存通过校验的迭代号。
  验证方法：`config.rs` 单测；连续创建 11 个迭代后检查配置文件。
- [ ] 从项目列表移除项目只改配置，不影响磁盘仓库；UI 文案明示。
  验证方法：真实窗口移除一个项目并保存，`ls` 该仓库仍存在。

## §2 创建

- [ ] 迭代号接受 `7.3.0` / `sprint-42`；拒绝空值、`.`、`..`、`a/b`、`a\b`、`a:b`、含 `* ? " < > |` 的名称、控制字符、`CON`、`con.txt`、`COM1`、`x.`（末尾句点）、`x `（末尾空格）。
  验证方法：`validation.rs::validate_iteration` 单测逐个用例；真实窗口输入 `..` 时「开始创建」禁用并显示原因。
- [ ] 分支名由 `git check-ref-format --branch` 校验；留空创建 Detached HEAD（清单 `headMode = detached`、`branch = null`）；本地已存在同名分支只使该项目失败且不建目录，其余项目继续。
  验证方法：S3 集成测试「同名本地分支已存在」；真实窗口创建时留空分支名，复核后状态列显示 `detached @ <短hash>`。
- [ ] 统一分支名与统一基分支单向实时联动：修改统一值覆盖所有已勾选行；新勾选项目继承当前统一值；单行编辑不反向影响统一值。
  验证方法：真实窗口操作；`test-frontend.mjs` 对 `CreateWorkspace` 的联动逻辑做 SSR 断言（若组件逻辑抽为 `utils` 函数则直接断言该函数）。
- [ ] 每个项目恰好 fetch 一次自己的基分支（`git fetch <remote> <branch>`），worktree HEAD 等于 `FETCH_HEAD^{commit}` 解析出的 Commit，清单 `baseCommit` 为 40 位 hex、`baseRef` 为归一化形式。
  验证方法：S3 集成测试「FETCH_HEAD 与 origin/master 一致」；`OperationView` 进度文案中每个项目只出现一条 `git fetch` 命令。
- [ ] 基分支留空＝`origin/master`；`origin/release`、`upstream/xxx`、裸分支名 `release` 都可用；打开创建页不发起任何远端连接；`ls-remote` 失败降级为手输且不阻断创建。
  验证方法：`base_ref.rs::normalize` 单测三态；真实窗口用一个远端不可达的仓库进入创建页不卡顿，点 `⟳` 后显示 warning 图标且仍能创建。
- [ ] 按 UI 顺序串行；部分失败后继续；单项目失败不回滚已成功项目；每个项目处理完立即持久化清单。
  验证方法：S3 集成测试「两项目中第二个远端分支不存在」+「每次项目处理后清单都是完整 JSON」。
- [ ] 有效现有 worktree（同项目同分支且 `active` 记录有效）再次创建显示「已存在」（`alreadyExists`），去重键＝项目＋分支，不含基分支；非管理同名目录冲突阻止该项目创建。
  验证方法：S3 集成测试「同项目同分支二次创建为 alreadyExists」；手工在迭代目录建一个同名普通目录后创建，该项目 `failed` 并给出冲突原因。
- [ ] 同迭代重复创建同名项目（不同分支）按 `p` / `p-2` / `p-3` 顺延，且把 `removed` / `createFailed` 记录也算作占用。
  验证方法：`path_utils.rs::next_directory_name` 单测；S3 集成测试「连续创建三个目录」；移除 `p-2` 后再创建得到 `p-4`。
- [ ] 在已隐藏迭代上继续创建会自动清 `hiddenAt`；创建请求缺省 `note` 不改动已有备注。
  验证方法：S6 单测；真实窗口先隐藏再创建，迭代回到主列表且备注保留。
- [ ] `OperationView` 逐项目显示阶段标签（queued / fetching / creating / vendor / completed / failed）与实际执行的命令文案；组件卸载后事件已 `unlisten`。
  验证方法：真实窗口观察；代码审查 `onUnmounted` 中调用 unlisten。

## §3 公共目录与 vendor

- [ ] 没有配置公共目录规则时创建正常进行，清单 `sharedDirectories` 为空数组。
  验证方法：真实窗口不配置规则直接创建；检查清单文件。
- [ ] 新迭代先按配置顺序完成全部公共目录，任一失败时清单对应规则 `status = failed`、`aborted = true`，且无项目进入 fetch / create。
  验证方法：S3 集成测试「公共目录失败时无项目进入 fetch」（源为符号链接）。
- [ ] 快照不含任意层级的 `.git` 目录或 `.git` 文件；不跟随符号链接（遇链接报错且不留半成品）；先复制到同父目录临时目录再 rename 落位；已存在且由清单管理的规则目标目录被复用（`status = reused`，不覆盖）。
  验证方法：`copy.rs` 四类单测；第二次向同迭代创建时清单显示 `reused`，目标目录 mtime 不变。
- [ ] 未被清单管理的同名目录或活动 worktree 与公共目录目标冲突时阻止创建，错误信息指出冲突路径。
  验证方法：手工在迭代目录建同名目录后创建，观察错误；S3 集成测试。
- [ ] 添加项目时识别 PHP（`composer.json`）/ Go（`go.mod`）/ 其他 / 未知，并显示 PHP 源仓库是否具备 `vendor`。
  验证方法：`config.rs::detect_project_type` 单测；真实窗口添加三类仓库观察类型列。
- [ ] vendor 六条件全覆盖：缺 `composer.json` → `notPhp`；源无 `vendor` → `sourceMissing`；任一端缺 `composer.lock` → `lockMissing`；lock Hash 不一致 → `lockMismatch`；目标已有 `vendor` → `targetExists`；全满足 → `copied` 且 `copiedByTool = true`。
  验证方法：`vendor.rs` 单测六条件逐条。
- [ ] Go 项目 vendor 列显示「无需复制」；lock 不匹配与复制失败都保留 worktree（`createResult.status = created`，`vendor.status` 记原因，`postSteps[vendor]` 为 `skipped` / `failed`）。
  验证方法：真实窗口观察；`vendor.rs` / `workspace.rs` 单测。
- [ ] 创建时重新检查源仓库真实 `vendor` 目录，不信任配置里的 `vendorAvailable` 快照。
  验证方法：配置保存后删除源仓库 `vendor`，再创建得到 `sourceMissing`。

## §4 清单与列表

- [ ] 清单写入采用同目录临时文件 + `flush` + `sync_all` + `rename`；写入失败不留临时文件。
  验证方法：`atomic_json.rs` 单测；创建过程中 `ls -a` 迭代目录观察临时文件消失。
- [ ] 启动只做快扫：存在的 worktree 状态列显示「未复核」，不跑任何 Git；复核只由「复核状态」按钮触发；页面无刷新按钮、无定时轮询。
  验证方法：真实窗口观察；代码审查 `list_workspaces(false)` 不调用 `git.rs`；`grep -rn setInterval src/` 无命中。
- [ ] 复核中旧列表保留、按钮 loading，完成后整体替换；复核持操作锁，复核期间发起创建得到「已有操作执行中」。
  验证方法：真实窗口；`lib.rs` 操作锁单测。
- [ ] 清单缺失显示「清单缺失」、损坏显示「清单损坏」并附原因；不改目录、不重建、不删除；该迭代「备注 / 隐藏 / 归档 / 检查合并」禁用，不能作为拖拽落点。
  验证方法：手工删除 / 写坏 `.worktree-manager.json` 后快扫；`manifest.rs::health_of` 单测；文件内容验证未被改写。
- [ ] `iteration` 与目录名不一致、`worktreePath` 重复、`schemaVersion ≠ 1` 都判 `damaged`。
  验证方法：`manifest.rs` 单测。
- [ ] 创建 / 移除 / 归档 / 排序 / 移动后列表立即（快扫）更新。
  验证方法：真实窗口逐个操作观察。
- [ ] 目录存在时「打开」按钮可用并用系统文件管理器（macOS `open`）打开；目录不存在时按钮禁用；`open_project` 后端校验路径在工作区根目录内。
  验证方法：真实窗口点击后 Finder 打开；`platform.rs` 单测参数向量；对越界路径调用 `open_project` 得 `validation` 错误。
- [ ] 复核能发现 discovered worktree（迭代目录下含 `.git` 文件且未在清单中的子目录），状态显示「未托管」；快扫不出现 discovered。
  验证方法：手工 `git worktree add` 一个目录到迭代目录，快扫看不到，复核后出现。
- [ ] 复核后 `validity` 正确：`valid` / `missingDirectory` / `notRegistered` / `headMismatch` / `sourceMissing` / `removed`。
  验证方法：`manifest.rs` 投影单测 `manifestHealth × validity` 组合；集成测试分别删目录、`git worktree prune` 后、切分支、移走源仓库。
- [ ] 列显隐（`worktree-manager.visible-columns`）、迭代折叠（`worktree-manager.expanded-iterations`）、隐藏区展开态（`worktree-manager.hidden-zone-expanded`）重启后保持；localStorage 损坏时回退默认并覆盖写入。
  验证方法：真实窗口操作后重启；`utils/storage.ts` 在 `test-frontend.mjs` 中的断言。
- [ ] `project` / `status` / `actions` 列不可隐藏；列顺序固定与 `ui-spec.md §4.1` 一致。
  验证方法：真实窗口列面板观察。
- [ ] 搜索按迭代号 / projectId / 分支名过滤主列表，不影响已隐藏收纳区。
  验证方法：真实窗口。

## §5 安全移除

- [ ] tracked 修改、普通 untracked 文件、未被 `refs/remotes` 引用的提交、Detached 独立提交、worktree `locked` 五类风险都阻止移除，`RemovalAssessment.allowed = false` 且 `risks[]` 含对应 `code`。
  验证方法：S3 集成测试（detached 独立提交、untracked）；`removal.rs` 单测覆盖 `trackedChanges` / `unpushedCommits` / `worktreeLocked`（用 `git worktree lock` 制造）。
- [ ] `prunable`、路径越界或未注册（`pathInvalid`）、清单损坏（`manifestInvalid`）同样阻止。
  验证方法：`removal.rs` 单测。
- [ ] 移除过程不执行 `--force`、`stash`、`commit`、`push`、分支删除。
  验证方法：代码审查 `git.rs` 白名单；`removal.rs` 参数向量单测中 `worktree remove` 不含 `--force`；`grep -rn '"stash"\|"push"\|"branch"' src-tauri/src` 无命中。
- [ ] 仅工具复制的 vendor（`vendor.copiedByTool = true` 且全部 untracked 路径以 `vendor/` 开头）为唯一 untracked 内容时 `vendorOnlyCleanupAvailable = true`；勾选并输入确认文本后先删 vendor 再执行不带 `--force` 的 `worktree remove`。
  验证方法：集成测试：创建 PHP 项目并复制 vendor → 评估 → `removeCopiedVendor = true` 移除成功；vendor 目录已删、worktree 已移除。
- [ ] `copiedByTool = false` 或 vendor 之外还有其他 untracked 时不提供 vendor 清理选项。
  验证方法：集成测试。
- [ ] 确认文本严格等于 `{iteration}/{目录名}`（区分大小写、无首尾空白容错）；不相等时拒绝且不动任何内容；执行前重新计算风险，评估后新增的风险能阻止执行。
  验证方法：`removal.rs` 单测；集成测试：评估后再写入一个 untracked 文件，执行移除被拒。
- [ ] 成功后保留本地分支、迭代目录、所有公共目录与清单历史（记录 `lifecycle = removed`、`removedAt` 非空），列表显示「已移除」。
  验证方法：集成测试后 `git branch --list` 仍有分支、`ls` 迭代目录与公共目录仍在、读清单。
- [ ] discovered worktree 的评估与移除以 `sourcePath` 为源仓库，风险规则相同，成功后不写清单记录。
  验证方法：集成测试 `assess_discovered_removal` / `remove_discovered_worktree`。
- [ ] 前端提交的 `worktreePath`、`removeCopiedVendor`、`confirmation` 都被后端重新校验，伪造路径得 `pathInvalid`。
  验证方法：单测直接调用服务函数传越界路径。

## §6 合并检查与归档（001 / 003 / 006 / 007 / 008）

- [ ] 真 merge → 层 1 `merged`；rebase 后合并 → 层 2 `merged`；squash 合并 → 层 3 `contained`；未合并 → `unmerged` 且 `unmergedCommits` 非空。
  验证方法：S5 集成测试三种 fixture + 未合并 fixture。
- [ ] 目标分支固定 `origin/develop` 与 `origin/master`；仓库缺该分支 → 该格 `targetMissing`，其它格与其它记录不受影响。
  验证方法：S5 集成测试「远端缺 develop」。
- [ ] 只查 `lifecycle = active`；`removed` / `createFailed` 记录不出现在 `records[]`。
  验证方法：S5 集成测试；真实窗口移除后再检查合并。
- [ ] 结果逐条通过 `merge-check-progress` 推送（`phase = record` 带 `record`），前端按 `worktreePath` 就地更新某一行，不等整批结束。
  验证方法：真实窗口：多项目迭代点「检查合并」观察逐行变化；代码审查事件处理器。
- [ ] 单仓库 fetch 失败降级为本地 `refs/remotes/origin/*` 缓存并将该仓库全部单元格标 `stale = true`，UI 显示「可能过时」，其它仓库继续。
  验证方法：S5 集成测试「fetch 失败（remote URL 指向不存在路径）」；真实窗口。
- [ ] `branch` 模式用 live 分支名；`detached` 用 HEAD commit；本地分支已删 → `branchMissing`；worktree 目录缺失 → `notCheckable`。
  验证方法：S5 集成测试 detached / 分支缺失路径。
- [ ] 「基准变动」按 `baseRef` 解析链（`baseRef` → 本地名 → `origin/master` → `master`）判定；rebase 到新基后不误报；基分支前进后不误报；列表优先采用合并检查返回的 `hasChanges` / `dirty`。
  验证方法：S5 集成测试 `hasChanges` 三种场景；真实窗口先复核再检查合并，观察列值切换。
- [ ] 合并检查结果只存内存（带 `checkedAt`），不落 localStorage、不落清单；切换页面回来后仍在，重启后消失。
  验证方法：代码审查；真实窗口重启后列为空。
- [ ] `MergeDetailDialog` 显示 `unmergedCommits` 列表与 `errorMessage`。
  验证方法：真实窗口点击 `unmerged` 单元格。
- [ ] 归档评估通过 `archive-progress` 逐条推送（`record = null`）；`clean` ＝ develop 与 master 均为 `merged | contained | targetMissing` 且 `dirty = false` 且 `stale = false`；`blockers` 文案由后端生成，前端不推导。
  验证方法：S6 单测 clean 四种组合；代码审查前端不含 clean 推导逻辑。
- [ ] 不干净且 `force = false` 时整体拒绝、不动任何记录；`ArchiveDialog` 显示干净 / 强制两态，强制态需勾选并输入迭代号。
  验证方法：S6 单测；真实窗口。
- [ ] 干净记录 `worktree remove` 不带 `--force`；`force = true` 且不干净的记录才带；确认文本严格等于迭代号。
  验证方法：`archive.rs` 参数向量单测。
- [ ] 全部成功才写 `archivedAt`；单条失败不中断整批，`ArchiveOutcome.failed[]` 列出失败项且 `archived = false`；每条成功后清单记录改 `removed`。
  验证方法：S6 单测「一条失败时 archived=false 且其余仍被移除」。
- [ ] `archivedAt` 非空的迭代快扫与复核都不出现；归档保留本地分支、迭代目录、公共目录与清单文件。
  验证方法：S6 单测 `list_groups` 跳过；真实窗口归档后 `ls` 目录仍在。

## §7 备注 · 隐藏 · 改名（009 / 010 / 011 / 012）

- [ ] 备注去首尾空白后 ≤50 个 Unicode 标量；含 `\n` / `\r` 拒绝；空串或纯空白＝清除（`note = null`）；返回值为规范化后的备注。
  验证方法：S6 单测 51 字符拒绝、换行拒绝、空串清除；`utils/note.ts` 前端预检在 `test-frontend.mjs` 断言。
- [ ] 创建请求 `note` 缺省（`undefined` / 字段不存在）时不改动已有备注；显式 `null` 或空串才清除。
  验证方法：`workspace.rs` 单测两种请求。
- [ ] 无备注时卡片头无任何常驻痕迹；有备注显示 `▤`；点 `✎ 备注` 内联编辑，回车保存、Esc 取消，输入框 `maxlength = 50`。
  验证方法：真实窗口。
- [ ] 备注气泡 `Teleport` 到 body、`position: fixed`；在折叠的迭代与已滚动的页面下都不被卡片 `overflow: hidden` 裁切；超出视口底部时翻到上方；滚动 / resize 立即收起；组件卸载时移除监听。
  验证方法：真实窗口把页面滚到底部 hover 最后一张卡片的 `▤`；代码审查 `onUnmounted`；`test-frontend.mjs` 断言关闭态不渲染内容。
- [ ] 隐藏只写 `hiddenAt`，不动目录与记录；迭代移入页面底部「已隐藏迭代（N）」收纳区，默认折叠；无隐藏项时整区不渲染；不受搜索影响；不渲染项目表格；「恢复」清 `hiddenAt` 回主列表。
  验证方法：S6 单测；真实窗口隐藏 / 搜索 / 恢复。
- [ ] `archivedAt` 优先于 `hiddenAt`：先隐藏再归档的迭代在主列表与收纳区都不出现。
  验证方法：集成测试；真实窗口。
- [ ] 备注 / 隐藏 / 归档都持操作锁；对 `manifestHealth ≠ valid` 的迭代调用得错误。
  验证方法：`lib.rs` 锁单测；对损坏清单调用 `set_iteration_note` 得 `manifestDamaged`。
- [ ] 本地改名判定：清单分支在源仓库已不存在、worktree 仍注册同路径且检出另一分支 → 复核后 `validity = valid`、`branchDisplay` 为 live 名、`renamedFrom` 为旧名，UI 显示「⟳ 已同步改名」，清单 `branch` 被原子回写且其余字段不变；toast「已同步 N 个分支重命名」（N = 0 不提示）。
  验证方法：S7 集成测试改名 fixture（`git branch -m`）；对比回写前后清单其它字段。
- [ ] 清单分支仍存在但与 live 不一致（切分支）→ `headMismatch` 且清单不动。
  验证方法：S7 集成测试切分支 fixture。
- [ ] 改名后合并检查与安全移除按 live 名判定，不再报 `branchMissing`。
  验证方法：S7 集成测试「改名后合并检查不报 branchMissing」。

## §8 基分支与拖拽（013 / 014）

- [ ] 进入创建页不发起任何远端连接；`includeRemote = false` 只读 `refs/remotes`。
  验证方法：`git.rs` 参数向量单测；用 remote URL 指向不可达地址的仓库进入创建页无延迟。
- [ ] `⟳` 触发 `ls-remote --heads <remote>`，15 秒超时且 `GIT_TERMINAL_PROMPT=0`；失败降级为本地候选并在 warning 图标 `title` 中给出脱敏后的原始报错，不阻断创建；整页「⟳ 全部」串行执行。
  验证方法：集成测试 `list_remote_branches` 失败路径；真实窗口。
- [ ] 归一化三态正确：空串 → `origin/master`；`upstream/xxx`（`upstream` 存在）原样；裸名 `release` 优先 `origin`，无 `origin` 且仅一个 remote 用之；remote 不存在或分支名非法 → 该项目失败且 `createFailed` 记录保留归一化结果（可得时）。
  验证方法：`base_ref.rs::normalize` 单测；S8 集成测试。
- [ ] 列表分支列下方显示「基于 {baseRef}」。
  验证方法：真实窗口。
- [ ] 拖拽手柄 `⠿` 是唯一 `draggable` 元素，`<tr>` 不设 `draggable`；点击复制路径 / 分支仍可用，单元格文本可选择；搜索过滤时手柄置灰不可拖。
  验证方法：真实 Tauri 窗口（不能只在浏览器验证）；代码审查。
- [ ] 落点语义为「插到锚点之前」，`null` ＝ 末尾；行上半部分插前、下半部分插后；落点行显示 2px 上边线；拖到原位（锚点为自身或自身下一行）不发请求。
  验证方法：`relocate.rs::reorder` 单测锚点语义；真实窗口 + 代码审查「原位不发请求」分支。
- [ ] 同迭代排序持久化到清单 `projects` 数组顺序，重启后保持。
  验证方法：真实窗口拖动后读清单文件并重启。
- [ ] 跨迭代移动真实 `git worktree move` 搬运本体；目标目录名按目标清单占用集合顺延（含 `removed` / `createFailed`）；先写目标清单再写源清单；两份清单都可读、`worktreePath` 不重复；移动后源与目标迭代的合并检查结论被清掉并重新快扫。
  验证方法：S8 集成测试「目标清单有 removed 同名记录时顺延 -2」「移动后两份清单无重复且目录真实搬走」；真实窗口跨卡片拖拽。
- [ ] 预检任一不通过即报错且不改动任何内容：清单不可读、源记录非 `active`、目录名不匹配 `projectId` / `projectId-N`、目录不存在或未注册、`locked`、`prunable`、目标迭代已归档、目标清单损坏。
  验证方法：S8 集成测试 `locked` / `prunable` 拒绝；`relocate.rs` 单测其它预检。
- [ ] 脏工作区移动只用单个 `-f`，禁止 `-f -f`。
  验证方法：`git.rs` 参数向量单测。
- [ ] 第二次清单写入失败时错误信息包含「点复核状态或手工修正清单」的恢复指引。
  验证方法：`relocate.rs` 单测用只读目录模拟源清单写入失败。
- [ ] 拖拽在真实窗口中光标正常、拖影跟随（`dragDropEnabled = false` 生效）。
  验证方法：真实 Tauri 窗口；检查 `tauri.conf.json`。

## §9 安全与并发

- [ ] `capabilities/default.json` 只含 `core:default` 与 `dialog:allow-open`；前端无通用 Shell / 文件系统权限。
  验证方法：文件检查。
- [ ] CSP 与 `architecture.md §4.1` 逐字一致；`index.html` 与构建产物无内联 `<script>` / `<style>`、无外部 URL。
  验证方法：`grep -rn "http" dist/` 只命中 `ipc.localhost`；查看 `tauri.conf.json`。
- [ ] 所有 Git 调用通过 `git.rs` 参数向量，`grep -rn 'Command::new' src-tauri/src` 只命中 `git.rs` 与 `platform.rs`；无 `sh -c` / `cmd /C`。
  验证方法：grep。
- [ ] 路径经 `canonicalize` 与边界验证；伪造的越界 `worktreePath` / `iteration` 对所有命令都被拒。
  验证方法：`path_utils.rs::ensure_within` 单测；对 `open_project` / `assess_removal` / `reorder_project` 传越界路径。
- [ ] 全局单操作锁：任意两个持锁命令并发时第二个立即得 `busy` 且 `message` 含当前操作名；不排队、不取消。
  验证方法：`lib.rs::OperationState` 单测；真实窗口复核中点创建。
- [ ] 用户可见 Git 错误经过 600 字符截断与凭证脱敏（`https://user:token@host` → `https://host`，`ssh://token@host` → `ssh://host`）。
  验证方法：`error.rs` 单测。
- [ ] porcelain / refs / worktree list 输出完整解析、不截断。
  验证方法：`git.rs` 解析单测含 50+ 条目输入。
- [ ] `@tauri-apps/api` 与 `plugin-dialog` 为动态 import；`test-frontend.mjs` 在 Node 下运行不访问 Tauri 全局对象。
  验证方法：`npm run test:frontend` 通过；代码审查 `api/tauri.ts`。

## §10 构建与发布

- [ ] `npm run typecheck`、`npm run test:frontend`、`npm run build`、`cd src-tauri && cargo test`、`npm run tauri build -- --no-bundle` 全部通过；`bundle.active = false` 且 `targets = []`，不生成任何 `.app` / `.dmg` / MSI / NSIS。
  验证方法：命令输出；`ls src-tauri/target/release/bundle` 不存在。
- [ ] 产物 `src-tauri/target/release/worktree-manager` 单独复制到空目录后可启动并加载主页面，能读到配置。
  验证方法：`implementation-plan.md §4` 便携验证命令。
- [ ] 依赖白名单：`Cargo.toml` 运行时依赖仅 `tauri` `tauri-plugin-dialog` `serde` `serde_json` `thiserror` `chrono` `sha2`（+ `cfg(windows)` 的 `windows-sys`）、构建依赖仅 `tauri-build`、dev 依赖仅 `tempfile`；`package.json` 无 UI / 状态 / 日期 / 图标 / CSS / 测试框架；版本为 `tauri 2.x`、`vite 7.x`、`typescript 5.9.x`。
  验证方法：文件检查；`npm ls --depth=0`；`cargo tree --depth 1`。
- [ ] 窗口标题 `Worktree Manager`、1440×900、最小 1080×700、可缩放、居中、`dragDropEnabled = false`。
  验证方法：`tauri.conf.json` 检查 + 真实窗口。
- [ ] README「实现状态」与「常用命令」段已用实测结果更新。
  验证方法：文件检查。

## §11 非本轮平台（默认「未验证（非本轮平台）」）

- [ ] Windows：`config_dir()` 为 `%APPDATA%\WorktreeManager`；`open_in_file_manager` 用 `explorer`；`is_link_like` 识别 reparse point；`windows-sys` 仅作为 `cfg(windows)` 依赖。（未验证：非本轮平台）
  验证方法：在 Windows 机器上运行 `cargo test` 与真实窗口；本轮只做 `cargo check --target x86_64-pc-windows-msvc`（若已安装 target）。
- [ ] Windows：`tauri build --no-bundle` 产出 `worktree-manager.exe`，复制到空目录可启动；WebView2 下拖拽光标与拖影正常。（未验证：非本轮平台）
  验证方法：Windows 机器。
- [ ] Linux：`config_dir()` 遵循 `XDG_CONFIG_HOME`；`open_in_file_manager` 用 `xdg-open`。（未验证：非本轮平台）
  验证方法：Linux 机器运行 `cargo test`。
- [ ] 跨平台通用规则在 macOS 上已生效：保留设备名拒绝、路径大小写不敏感比较、`dragDropEnabled = false`。（此条属于本轮，应在 S4/S8 验证）
  验证方法：`validation.rs` / `platform.rs` 单测；真实窗口拖拽。

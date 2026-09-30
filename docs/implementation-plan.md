# Worktree Manager · 实施计划

> 交给实现方（单个 AI，串行推进）的执行手册。每一步的「完成定义」未全部满足不得进入下一步。「可并行」标注仅供参考：若实现方具备并行能力，可按依赖图拆分；否则按步骤号顺序执行。

---

## 0. 工作方式（不可协商）

1. **先设计再实现**：每个功能开工前先读 `docs/design/NNN-*/design.md`；实现完成后在该文档末尾追加「实现记录」小节（做了什么、偏离了什么、为什么）。**不回改设计正文**。
2. **禁止虚报**：报告的每一条检查必须真实执行过；无法执行的写「未执行 / 无法验证」及原因。
3. **依赖白名单**：见 `architecture.md §1`。
4. **语言**：注释、文档、UI 文案、错误信息中文；标识符与 Git 术语英文。
5. **不擅自扩大范围**：`todo.md` 里「不做」的东西发现了只记录。
6. **一次只推进一个功能**。
7. **每步结束用 §7 的汇报格式汇报**。

---

## 1. 依赖图

```text
S0 文档基线（已完成，实现方只需阅读）
 │
S1 工程骨架 + 视觉静态还原 ─────────────┐
 │                                        │  （S1 与 S2 互不依赖，可并行）
S2 Rust 领域层（纯函数 + 单测）───────────┘
 │
S3 创建与移除闭环（MVP 可运行）  ← 依赖 S1 的 api/tauri.ts 骨架 + S2 全部模块
 │
S4 MVP 验收 + 便携构建 + README
 │
S5 001-007 合并检查族            ← 依赖 S3 的复核与清单投影
 │   ├─ 002 剪贴板（纯前端，可与 001 并行）
 │   └─ 004 快扫/复核分档（S3 已打底，此处补锁与 UI）
S6 008-010 归档 / 备注 / 隐藏    ← 008 依赖 S5 的 merge_check.rs；009/010 只依赖 S3（可与 S5 并行）
 │
S7 011-012 改名同步 / 备注气泡   ← 011 依赖 S3 复核；012 依赖 S6 的 009（可与 S6 后半并行）
 │
S8 013-014 基分支 / 拖拽移动     ← 013 依赖 S3 创建流程；014 依赖 S3 清单 + S5 的结果清理钩子
 │
S9 全量验收 + 文档一致性审计
```

**串行执行顺序即步骤号顺序。**

---

## 2. 各步骤任务

### S1 · 工程骨架与视觉静态还原

**目标**：`npm run dev` 能在浏览器看到三页三模态的静态样子（假数据），`cargo check` 通过，`tauri build --no-bundle` 能产出可启动的空壳。

**任务**：

1. `npm create tauri-app` 等价的手工初始化（不要依赖脚手架的额外依赖）：`package.json`（scripts 与依赖版本按 `architecture.md §1`）、`vite.config.ts`、`tsconfig.json`、`index.html`（无内联脚本样式）。
2. `src-tauri/`：`Cargo.toml`（白名单 + release profile）、`build.rs`、`tauri.conf.json`（窗口标题 `Worktree Manager`、1440×900、最小 1080×700、可缩放、居中、`dragDropEnabled: false`、CSP、`bundle.active: false`、`bundle.targets: []`、`identifier: com.worktreemanager.app`）、`capabilities/default.json`、`main.rs`、`lib.rs`（注册 dialog 插件，`generate_handler![]` 为空）。
3. `src/styles.css` 按 `ui-spec.md §1-6` 写全 token 与基础样式。
4. `src/types.ts` 按 `data-model.md` 写全类型。
5. `src/api/tauri.ts` + `src/api/demo-data.ts`：19 个封装函数与 3 个事件订阅，浏览器演示模式返回假数据；Tauri 模式此时可以 `invoke` 同名命令（尚未注册也无妨）。
6. 组件全部建出并用假数据静态还原：`App.vue`、`AppSidebar`、`WorkspaceList`、`IterationCard`、`CreateWorkspace`、`SettingsView`、`OperationView`、`RemovalDialog`、`MergeDetailDialog`、`ArchiveDialog`、`NoteTooltip`、`ToastStack`；`utils/*.ts` 至少有文案映射与 localStorage 读写。
7. `scripts/test-frontend.mjs`：用 Vite `createServer({ server: { middlewareMode: true }, appType: 'custom' })` + `ssrLoadModule` 加载组件与 utils，用 `createSSRApp` + `renderToString` 断言关键文案（如列表页渲染出「工作区」「复核状态」、状态标签文案映射正确、`note.ts` 校验函数行为）。至少 8 个断言，全部 `assert/strict`。测试结束 `await server.close()` 并 `process.exit(0)`。

**完成定义**：`npm run typecheck`、`npm run test:frontend`、`npm run build`、`cd src-tauri && cargo check`、`npm run tauri build -- --no-bundle` 全部通过；启动产物能看到列表页假数据。

### S2 · Rust 领域层（纯函数 + 单测）

**目标**：不依赖 Tauri 运行时的全部基础模块与测试。

**任务**（每个模块先写签名与文档注释，再写实现，再写测试）：

| 模块 | 必写内容 | 必写单测 |
| --- | --- | --- |
| `error.rs` | `AppError` 枚举、`to_object()`、`redact_credentials()`、`truncate_message()` | `https://user:token@host/x` → `https://host/x`；`ssh://token@host` → `ssh://host`；601 字符截断 |
| `models.rs` | 全部结构与枚举 + serde 属性 | 每个枚举 round-trip；`notRequired` 可反序列化；`workspaceRoot: null` 可反序列化 |
| `platform.rs` | `config_dir`、`open_in_file_manager`（返回参数向量供测试）、`is_link_like`、`paths_equal` | `config_dir` 以 `WorktreeManager` 结尾；`paths_equal("A/b","a/B")` 为真 |
| `validation.rs` | `validate_iteration`、`validate_directory_name`、`validate_project_id`（含冲突集合）、`validate_branch_name`（本地规则 + 由调用方追加 `check-ref-format`）、`validate_absolute_path` | `7.3.0`/`sprint-42` 通过；`..`/`a/b`/`a\b`/`a:b`/`CON`/`con.txt`/`x.`/`x `/空串/`\u{1}` 拒绝；标识大小写不敏感重复、与公共目录目标冲突拒绝 |
| `path_utils.rs` | `canonicalize_existing`、`resolve_allow_missing`（找最近祖先）、`ensure_within(root, path)`、`join_segments`、`next_directory_name(existing_names, project_id)` | 越界拒绝；允许缺失解析出预期路径；`next_directory_name` 在占用集合含 `p`、`p-2` 时返回 `p-3`（占用集合由调用方传入，包含 `removed`/`createFailed` 记录） |
| `atomic_json.rs` | `read_json<T>`、`write_json_atomic<T>` | 写后可读；写入失败（目标目录只读）不留临时文件；损坏文件读取报错且文件内容不变 |
| `config.rs` | `load`（缺省默认）、`migrate_legacy`、`normalize_and_validate`、`save`、`detect_project_type`、`resolve_projects` | 迁移 `fdCommonSource`；丢弃 `goCommonSource` 与 `enabled=false`；`workspaceRoot` 空串→`null`；`recentIterations` 去重限 10 |
| `git.rs` | `GitRunner { repo: PathBuf }` + 每个白名单子命令一个方法；`GitArgs` 构造函数与执行分离；`run` 与 `run_with_timeout`（`spawn` + `try_wait` 轮询 + 超时 `kill`）；`worktree_move(old, new, force: bool)`；`parse_worktree_list`、`parse_status`；错误脱敏 | 每个子命令参数向量逐条比对（含 `force=true` 只出现一个 `-f`）；`run_with_timeout` 对 `sleep 5` 类命令 1 秒超时返回错误；porcelain 解析：正常 / `locked` / `prunable` / detached / 多 worktree；status 解析：tracked 修改、untracked、重命名、子目录 |
| `copy.rs` | `copy_snapshot(src, dst)`：跳过任意层级 `.git`（目录与文件）、遇链接返回错误、先复制到同父目录 `.tmp-*` 再 rename、失败清理 | 四类：含 `.git` 目录与 `.git` 文件被排除；符号链接拒绝且无半成品；目标已存在拒绝；中途失败（用只读子目录模拟）不留半成品 |
| `vendor.rs` | `assess_vendor(worktree, source_repo) -> VendorDecision`、`copy_vendor`、`lock_hash(path)` | 六条件逐条缺失分别得到 `notPhp`/`sourceMissing`/`lockMissing`/`lockMismatch`/`targetExists`；全满足得 `copied` 决策 |
| `manifest.rs`（纯部分） | `Manifest` 读写、`health_of(dir)`、`project_snapshot(...)`（快扫投影）、`directory_name_of(path)`、`occupied_names()` | `manifestHealth × validity` 组合：valid+目录存在→`unknown`；valid+缺目录→`missingDirectory`；removed→`removed`；missing/damaged→`projects` 为空 |

**完成定义**：`cargo test` 通过且以上单测全部存在；`cargo clippy`（若可用）无 error；`cargo check --target x86_64-pc-windows-msvc` 若本机已安装该 target 则执行，否则记「未执行」。

### S3 · 创建与移除闭环（MVP 可运行）

**目标**：真实 Tauri 窗口里能配置、创建、看列表、复核、打开目录、安全移除。

**任务**：

1. `workspace.rs::create_batch(config, request, emit)`：按判定口径 1-4（见 `requirements.md §3`）编排：校验 → 迭代目录与清单初始化（已隐藏迭代自动清 `hiddenAt`；`note` 缺省不改）→ 公共目录全部完成 → 逐项目 `fetch` → `rev-parse FETCH_HEAD` → 冲突检查（同项目同分支已存在 `active` 记录且有效 → `alreadyExists`；本地同名分支存在 → 失败；未托管同名目录 → 失败）→ `worktree add` → vendor → 每项目后原子写清单 → `create-progress`。
2. `manifest.rs` 完整复核：`list_groups(config, reconcile)`；discovered 扫描；调用 `head_state.rs` 的骨架（S7 再实现改名判定，此处只判 `valid/headMismatch`）。
3. `removal.rs::assess(...)` 与 `remove(...)`：风险五类 + vendor 唯一 untracked 判定 + 确认文本 + 执行前重算。discovered 版本同理（以 `sourcePath` 为源仓库）。
4. `lib.rs`：`OperationState`、MVP 11 个命令注册（`get_config` `save_config` `resolve_projects` `list_workspaces` `create_workspaces` `assess_removal` `remove_worktree` `assess_discovered_removal` `remove_discovered_worktree` `open_iteration` `open_project`）。
5. 前端接通：设置页保存/解析、创建页与 `OperationView`、列表页快扫/复核/打开/移除对话框、顶部横幅「浏览器演示模式」。

**集成测试**（`src-tauri/tests/` 或模块内 `#[cfg(test)]`，用 `tempfile` + 本地裸仓库作 remote；本机无 `git` 时打印原因并 `return`，**不得**让生产代码跳过校验）：

- `FETCH_HEAD` 解析的 commit 与远端 `master` 一致，且新 worktree HEAD 相等。
- 两项目，第二个的远端分支不存在 → 第一个 `created`、第二个 `failed`，清单两条都在。
- detached 独立提交阻止移除（`detachedCommits`）。
- 普通 untracked 文件阻止移除；删除后可移除。
- 同名本地分支已存在 → 该项目失败且不建目录。
- 连续创建同项目三次（不同分支）目录为 `p`/`p-2`/`p-3`；同项目同分支二次创建 → `alreadyExists`。
- 公共目录失败（源为符号链接）→ 无项目进入 fetch，清单 `sharedDirectories[].status = failed`。
- 每次项目处理后清单都是完整可解析 JSON（用测试钩子在每步后读文件）。

**完成定义**：以上测试通过；真实窗口手工验证：设置 → 创建两个仓库 → 列表显示「未复核」→ 复核变「有效」→ 移除一个成功。`cargo test`、前端三命令、`tauri build --no-bundle` 全过。

### S4 · MVP 验收与便携构建

1. 逐条验证 `acceptance.md` 的 §1-§5（配置 / 创建 / 公共目录与 vendor / 清单与列表 / 安全移除）。
2. `npm run tauri build -- --no-bundle` → 把 `src-tauri/target/release/worktree-manager` 复制到空临时目录启动，确认主页面加载并能读到配置。
3. 更新 `README.md`：把「常用命令」「数据位置」「构建产物」段落补成实测结果（体积、路径）。
4. 把发现的、不属于 MVP 的问题写进 `docs/todo.md`「实现期间发现」小节。

**完成定义**：`acceptance.md` §1-§5 全部打勾或标明「未验证 + 原因」。

### S5 · 001-007 合并检查族

设计文档：`design/001` `002` `003` `004` `005` `006` `007`。

1. `merge_check.rs`：`check(config, iteration, filter, emit)`；每仓库 `fetch --no-tags origin develop master`（失败→ 该仓库全部 `stale=true`，改用本地 `refs/remotes/origin/*`）；三层判定；只查 `active`；逐条 `merge-check-progress`；结果带 `checkedAt`，不落盘。
2. `hasChanges`（005）：解析链 `baseRef` → 去 remote 前缀本地名 → `origin/master` → `master`，`rev-list --count <base>..HEAD > 0`；discovered 用 `origin/master` 否则 `master`。复核档的列表也用同一函数。
3. `MergeRecordResult` 带 `hasChanges`/`dirty`（006）；前端 `WorkspaceList` 按 `worktreePath` 就地更新行。
4. 002：`utils/clipboard.ts`（`navigator.clipboard.writeText` 失败降级到隐藏 `textarea` + `document.execCommand("copy")`），单击整格复制，无后端命令，不占锁。
5. 004：确认 `list_workspaces(true)` 持锁；复核中按钮 loading、旧列表保留。
6. `check_merge_status` 注册；`MergeDetailDialog` 显示 `unmergedCommits` 与 `errorMessage`。

**测试**（`tempfile` + 裸仓库 fixture）：真 merge → 层 1 `merged`；rebase 后合并 → 层 2 `merged`；squash 合并 → 层 3 `contained`；未合并 → `unmerged` 且 commit 列表非空；远端缺 `develop` → `targetMissing`；fetch 失败（remote URL 指向不存在路径）→ `stale`；detached 记录 → 按 HEAD commit 判定；本地分支已删 → `branchMissing`；操作锁互斥（第二个 `acquire` 得 `busy`）。

**完成定义**：以上测试通过；真实窗口：点「检查合并」后逐行变化；`acceptance.md` §6 前三条打勾。

### S6 · 008-010 归档 / 备注 / 隐藏

设计文档：`design/008` `009` `010`。

1. `archive.rs::assess`：持操作锁（操作名「评估归档」）；复用 `merge_check`（`archive-progress` 事件，`record=null`），每条 `clean = develop,master ∈ {merged,contained,targetMissing} && !dirty && !stale`；`blockers` 文案由后端生成。
2. `archive.rs::archive`：校验 `confirmation == iteration`；**内部重新评估**（不信任前端传来的 clean 状态）；`!clean && !force` → 拒绝；逐条 `worktree remove`（干净不带 `--force`，`force=true` 且不干净才带）；单条失败不中断；全部成功才写 `archivedAt`；每条后原子写清单（`lifecycle=removed`、`removedAt`）。
3. `list_groups` 跳过 `archivedAt` 非空。
4. `set_iteration_note`：trim、≤50 标量、含 `\n`/`\r` 拒绝、空串→`null`。创建请求 `note` 缺省不改动。
5. `set_iteration_hidden`：写/清 `hiddenAt`；创建时自动清。
6. 前端：`ArchiveDialog` 两态；备注内联编辑；隐藏收纳区。

**测试**：clean 判定四种组合；`force=false` 不干净拒绝；一条失败时 `archived=false` 且其余仍被移除；`note` 51 字符拒绝、含换行拒绝、空串清除；隐藏/恢复时间戳；`archivedAt` 迭代不在 `list_groups` 输出。

### S7 · 011-012 改名同步 / 备注气泡

设计文档：`design/011` `012`。

1. `head_state.rs::judge(record, worktree_entry, source_repo)`：清单 `branch` 在源仓库 `show-ref` 不存在 && 同路径注册 && live 分支存在且不同 → `Renamed { live }`；清单分支仍存在但 live 不同 → `HeadMismatch`；一致 → `Valid`。复核时对 `Renamed` 原子回写清单 `branch`（只改该字段，其余字节级不变的语义：重新序列化整份清单但不改其他字段）。前端按 `renamedFrom` 非空行数统计 N 供 toast，命令签名不变。
2. 合并检查与移除按 live 名判定（读清单后先经 `head_state`）。
3. `NoteTooltip.vue` 按 `ui-spec.md §7.4`（`scroll` 与 `resize` 都以 `capture: true` 监听并收起）。

**测试**：改名 fixture → `valid` + `renamedFrom` + 清单 `branch` 已更新；切分支 fixture → `headMismatch` 且清单不变；改名后合并检查不报 `branchMissing`。前端 SSR 测试：`NoteTooltip` 关闭态不渲染内容。

### S8 · 013-014 基分支 / 拖拽移动

设计文档：`design/013` `014`。

1. `base_ref.rs::normalize(repo_remotes, input) -> Result<String>`：空串→`origin/master`；`<remote>/<branch>` 首段命中 remote 则原样；裸名优先 `origin`，无 `origin` 且仅一个 remote 用之；否则错误。`list_remote_branches`：`includeRemote=false` 只读 `refs/remotes`；`true` 走 `ls-remote`（15 秒超时），失败降级并给 `warning`。
2. 创建页：统一/逐项目基分支 `<datalist>`；`⟳` 单项与「全部」串行；列表「基于 XXX」。
3. `relocate.rs::reorder(iteration, worktree_path, before)` 与 `move_project(request)`：预检（两份清单可读、源 `active`、目录名匹配 `projectId`/`projectId-N`、目录存在且注册、未 `locked`、非 `prunable`、目标非归档、目标名按占用集合顺延）→ `worktree move [-f]`（脏用单个 `-f`）→ 先写目标清单再写源清单。
4. 前端拖拽按 `ui-spec.md §7.5`；成功后清合并结论并重载。

**测试**：归一化三态 + 错误；`reorder` 锚点语义（before=null 追加、before=首行插头、原位不变）；`move` 目标名在目标清单有 `removed` 同名记录时顺延 `-2`；`locked` 拒绝；`prunable` 拒绝；移动后两份清单 `worktreePath` 无重复且目录真实搬走。
**必须在真实窗口验证拖拽**：光标、拖影、落点指示、跨卡片落点。

### S9 · 全量验收与文档审计

1. 逐条过 `acceptance.md` 全部章节。
2. 执行全部构建自检命令并记录输出摘要。
3. 文档一致性审计：`architecture.md`、`data-model.md`、`workflows.md` 与代码逐节对照，枚举与 `models.rs` 逐个对照；输出格式 `<文件> §<章节>：<问题> → <修正>`，无问题写「无偏差」。
4. 输出交付物清单（见 §6）。

---

## 3. 可并行项一览（供具备并行能力的实现方参考）

| 组 | 可同时进行 | 前置 |
| --- | --- | --- |
| A | S1 前端静态还原 ∥ S2 Rust 领域层 | S0 |
| B | S3 中 `workspace.rs` ∥ `removal.rs` ∥ 前端接线（接口已冻结） | S2 |
| C | S5 的 `merge_check.rs` ∥ 002 剪贴板 ∥ `MergeDetailDialog` | S3 |
| D | S6 的 009/010 ∥ S5 | S3 |
| E | S7 的 012 气泡 ∥ S7 的 011 | S6(009) / S3 |
| F | S8 的 013 ∥ 014 | S3 / S5 |
| G | 14 份设计文档的「实现记录」 | 各自功能完成 |

硬依赖：S3 依赖 S2 全部；008 依赖 001；014 前端「清结论」依赖 003 的内存结构；011 影响 001/008/移除的分支名来源，需在 S7 后回归 S5/S6 测试。

---

## 4. 自检命令（每步结束必跑）

```bash
npm install
npm run typecheck
npm run test:frontend
npm run build
(cd src-tauri && cargo test)
npm run tauri build -- --no-bundle
# 便携验证（macOS）
dst="$(mktemp -d)/wm-portable-check"; mkdir -p "$dst"
cp src-tauri/target/release/worktree-manager "$dst/"
"$dst/worktree-manager" &
```

集成测试需要 `git`；本机没有时测试 `return` 并打印原因，生产代码不跳过任何校验。

---

## 5. 每步汇报格式

```text
## 完成的功能
- <编号/名称>：<一句话>（新增/修改文件：<路径>）
## 我实际执行的命令
- <命令> → <结果：通过/失败/未执行（原因）>
## 与设计的偏离
- <无 / 具体偏离 + 原因 + 写入了哪个设计文档的「实现记录」>
## 未验证项
- <条目 + 原因 + 请用户如何验证>
## 下一步
- <下一步编号与内容>
```

---

## 6. 最终交付物清单

```text
代码：src-tauri/src/*.rs（模块数与行数）、src/**（组件数与行数）、scripts/test-frontend.mjs
文档：README.md、AGENTS.md、docs/{requirements,architecture,workflows,data-model,ui-spec,
      implementation-plan,acceptance,todo}.md、docs/design/001..014/design.md（含实现记录）
产物：src-tauri/target/release/worktree-manager（路径、体积、构建时间、平台）
验证：cargo test / typecheck / test:frontend / build / 空目录启动 / 真实窗口拖拽 的结果摘要
审计：文档一致性审计输出
```

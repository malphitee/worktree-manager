# Worktree Manager · 总体架构设计

> 本文是系统**当前态的单一事实来源**之一（另两份是 `data-model.md` 与 `workflows.md`）。功能合入时必须同步更新对应章节。
> 本文由 `docs.md` 原始规格整理而来，并按以下已确认决策做了调整：
> 1. 目标平台改为**当前开发平台（macOS 优先）**，代码保持跨平台，Windows / Linux 分支用 `cfg` 保留但不作为本轮验收目标。
> 2. **不预设任何路径**：工作区根目录、公共目录规则默认为空，用户首次在设置页填写后才能创建。
> 3. 实现方为**单个 AI 串行推进**，实施计划中的「可并行」仅供参考。
> 4. 文档集（含 14 份功能设计文档）已一次性写完，实现方只在设计文档追加「实现记录」。

---

## 0. 一句话定位

面向单个开发者的本地桌面工具：按迭代号（如 `7.3.0`）把多个 Git 仓库的 worktree 统一创建到 `{工作区根目录}/{迭代号}/{项目}` 下，复制公共目录快照与 PHP `vendor`，并在一个界面里复核状态、检查合并、安全移除、归档整个迭代。

**不是**：CI 工具、Git 客户端、团队协作工具。不联网（除 `git fetch` / `git ls-remote`）、不登录、不同步。

---

## 1. 技术栈与版本锁定

| 层 | 选型 | 锁定版本（`package.json` / `Cargo.toml` 必须显式钉住） |
| --- | --- | --- |
| 桌面框架 | Tauri v2 | `tauri = "2"`（**禁止** 3.x alpha）、`@tauri-apps/cli ^2`、`@tauri-apps/api ^2`、`@tauri-apps/plugin-dialog ^2`、`tauri-plugin-dialog = "2"` |
| 后端 | Rust edition 2021，`rust-version = "1.77.2"` | 运行时依赖**仅**：`tauri`、`tauri-plugin-dialog`、`serde`（derive）、`serde_json`、`thiserror`、`chrono`（`clock`,`serde`）、`sha2`；`[target.'cfg(windows)'.dependencies]` 下 `windows-sys`（`Win32_Foundation`,`Win32_Storage_FileSystem`）；`[build-dependencies]` 仅 `tauri-build`；`[dev-dependencies]` 仅 `tempfile` |
| 前端 | Vue 3.5 + TypeScript 5.9 + Vite 7 | `vue ^3.5`、`typescript ~5.9`（**禁止** 7.x）、`vite ^7`（**禁止** 8.x）、`@vitejs/plugin-vue ^6`、`vue-tsc ^3` |
| 测试 | Rust `cargo test`；前端 `scripts/test-frontend.mjs` | 不引入任何测试框架 |

**依赖白名单是硬约束**：不引入 UI 框架、状态管理库、日期库、图标库、CSS 框架、`dirs`/`open`/`regex` 等便利 crate。所有平台差异用 `std::env` + `cfg` 自行处理。

`package.json` scripts 固定：

```json
{
  "dev": "vite",
  "test:frontend": "node scripts/test-frontend.mjs",
  "typecheck": "vue-tsc --noEmit",
  "build": "vue-tsc --noEmit && vite build",
  "tauri": "tauri"
}
```

Vite：端口 `1420`、`strictPort: true`、`clearScreen: false`。
`tsconfig.json`：`strict`、`noUnusedLocals`、`noUnusedParameters`、`verbatimModuleSyntax`、`moduleResolution: "bundler"`、`jsx: "preserve"`、`target: ES2022`。
Cargo `[profile.release]`：`lto = true`、`codegen-units = 1`、`opt-level = "s"`、`strip = true`、`panic = "abort"`。

---

## 2. 平台策略（本轮最重要的调整）

### 2.1 验收平台

- **主验收平台：macOS（aarch64-apple-darwin）**。全部单测、集成测试、构建、真实窗口交互（含拖拽）都在 macOS 上执行并报告。
- Windows / Linux：代码路径保留，`cargo check` 级别保证编译逻辑正确，但所有依赖真实运行的验收条目标记「未验证（非本轮平台）」。

### 2.2 平台抽象模块 `platform.rs`

所有平台差异集中在 `src-tauri/src/platform.rs`，其他模块**不得**直接写 `cfg(windows)` / `cfg(unix)`（`validation.rs` 中的保留名规则除外，那是跨平台通用规则）。

| 函数 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| `config_dir() -> PathBuf` | `$HOME/Library/Application Support/WorktreeManager` | `%APPDATA%\WorktreeManager` | `$XDG_CONFIG_HOME/WorktreeManager`，缺省 `$HOME/.config/WorktreeManager` |
| `open_in_file_manager(path)` | `open <path>` | `explorer <path>` | `xdg-open <path>` |
| `is_link_like(&Metadata) -> bool` | `file_type().is_symlink()` | 同左 **或** `FILE_ATTRIBUTE_REPARSE_POINT`（`windows-sys`） | 同 macOS |
| `paths_equal(a, b) -> bool` | 大小写不敏感比较规范化后的字符串 | 同左 | 同左（刻意与其他平台一致，见 2.3） |
| `is_absolute(path)` | `Path::is_absolute()` | `Path::is_absolute()`（含盘符与 UNC） | 同 macOS |

均通过 `std::process::Command` 参数向量调用，不拼接 shell 字符串。

### 2.3 保留的「Windows 特化」规则（跨平台统一执行）

以下规则在原规格中是 Windows 踩坑结论，本轮**全平台统一保留**，理由是工作区目录可能通过同步盘/外置盘在系统间流转，统一严格规则成本极低：

- 迭代号 / 项目标识 / 公共目录目标名校验：拒绝空值、`.`、`..`、路径分隔符（`/` 与 `\` 都拒绝）、非法字符 `\ / : * ? " < > |`、控制字符、Windows 保留设备名（`CON` `PRN` `AUX` `NUL` `COM1-9` `LPT1-9`，大小写不敏感，含带扩展名形式如 `con.txt`）、末尾空格或句点。
- 文件系统比较一律大小写不敏感（macOS APFS 默认亦不区分大小写）。
- `app.windows[].dragDropEnabled = false`（macOS WKWebView 同样存在系统拖放抢占 HTML5 拖拽的问题，关闭无副作用）。

### 2.4 默认配置

- `workspaceRoot: null`、`sharedDirectories: []`、`projects: []`、`recentIterations: []`。
- 配置文件不存在时返回上述默认值，**不创建文件**；只有用户保存后才写盘。
- `workspaceRoot` 为 `null` 时：`list_workspaces` 返回空数组；`create_workspaces` 返回校验错误「请先在设置中填写工作区根目录」；设置页对该字段显示「必填」标记。
- 不再内建 `fd-common` 保留字。项目标识只需与**已配置**的公共目录目标名不冲突。旧格式 `fdCommonSource` 仍迁移为一条目标为 `fd-common` 的规则。

### 2.5 构建产物

- `tauri.conf.json`：`bundle.active = false`、`bundle.targets = []`。
- `npm run tauri build -- --no-bundle` 在 macOS 产出 `src-tauri/target/release/worktree-manager`（Mach-O 可执行文件）；Windows 上则为 `.exe`。
- 「便携验证」＝把该可执行文件单独复制到一个空目录并启动，主页面能加载。

---

## 3. 目录与模块职责

```text
src-tauri/
  tauri.conf.json      窗口 + CSP + bundle.active=false + dragDropEnabled=false
  Cargo.toml           依赖白名单 + release profile
  capabilities/default.json   仅 core:default + dialog:allow-open
  build.rs             tauri_build::build()
  src/
    main.rs            仅调用 worktree_manager_lib::run()
    lib.rs             Tauri 装配、Command 适配层、generate_handler!、OperationState
    models.rs          全部序列化结构与枚举（与 data-model.md 逐一对应）
    error.rs           AppError → 稳定错误对象（code/message，长度限制与凭证脱敏）
    platform.rs        §2.2 平台抽象
    validation.rs      迭代号、项目标识、目录名、分支名、绝对路径校验
    path_utils.rs      canonicalize、允许缺失的解析、边界验证、逐段 join、目录名顺延
    atomic_json.rs     原子 JSON 读写
    config.rs          配置定位、旧格式迁移、校验、normalize_and_save、项目解析
    git.rs             Git 子命令封装（参数向量 + 白名单）、porcelain 解析、错误脱敏
    copy.rs            公共目录快照复制（排除 .git、不跟随链接、临时目录 + rename）
    vendor.rs          composer.lock SHA-256 判定与 vendor 复制
    manifest.rs        清单读写、状态投影、discovered 扫描、快扫/复核
    head_state.rs      分支 live 状态与「本地改名」判定（011）
    workspace.rs       串行创建编排 + create-progress 事件
    removal.rs         风险检查与安全移除编排
    merge_check.rs     三层合并判定（001/003/005/006/007）
    archive.rs         归档评估与批量移除（008）
    relocate.rs        排序与跨迭代移动（014）
    base_ref.rs        基分支归一化与远端分支列举（013）
src/
  main.ts              createApp + 挂载 + 引入 styles.css
  App.vue              应用壳：页面状态机（workspaces | create | settings）、侧栏、顶部错误横幅、toast、模态层挂载点
  styles.css           CSS token 与基础样式（见 ui-spec.md）
  types.ts             与 Rust 结构一一对应的 TS 类型
  api/tauri.ts         全部 invoke 与 listen 的唯一出口；浏览器构建下提供只读演示回退
  api/demo-data.ts     浏览器演示模式用的假数据
  components/
    AppSidebar.vue     导航「工作区」「设置」
    WorkspaceList.vue  迭代卡片列表 + 列显隐 + 搜索 + 已隐藏收纳区 + 拖拽
    IterationCard.vue  单个迭代卡片（头部按钮组、项目表格）
    CreateWorkspace.vue 创建页（迭代号、最近迭代胶囊、统一分支/基分支、项目勾选表、底部固定操作条）
    SettingsView.vue   设置页（根目录、公共目录规则表、项目表）
    OperationView.vue  创建进度视图（逐项目阶段）
    RemovalDialog.vue  移除确认模态（风险列表、vendor 清理、确认文本）
    MergeDetailDialog.vue 合并详情模态（未合并 commit 列表）
    ArchiveDialog.vue  归档模态（干净/强制两态）
    NoteTooltip.vue    备注气泡（Teleport）
    ToastStack.vue     右上角 toast
  utils/
    base-ref.ts        基分支归一化预览（仅展示，不做安全判定）
    config.ts          设置表单 ↔ AppConfig 转换、前端侧格式预检
    merge.ts           MergeCellStatus → 标签文案/色
    note.ts            备注长度/换行预检
    status.ts          validity/vendorStatus/lifecycle → 标签文案/色
    clipboard.ts       navigator.clipboard + execCommand 降级
    storage.ts         localStorage 键读写
scripts/test-frontend.mjs   Vite createServer + createSSRApp + renderToString + assert/strict
```

**Command 适配层（lib.rs）只做**：反序列化参数 → 获取操作锁（若需要）→ 调服务函数 → 错误转换 → 长任务发射进度事件。**不写业务规则**。

---

## 4. 安全与信任模型

### 4.1 能力边界

- 不启动 HTTP 服务；不暴露通用 Shell 或任意文件系统接口。
- `capabilities/default.json` 只允许 `core:default` 与 `dialog:allow-open`（目录多选）。
- CSP 严格为：
  `default-src 'self'; img-src 'self' asset: data:; style-src 'self'; script-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'`
  脚本与样式全部随应用打包，无 CDN，无内联 `<style>` / `<script>`（Vue SFC 样式由 Vite 抽成 `.css`）。

### 4.2 路径信任

- 配置里的仓库路径与公共目录源路径在保存与使用前都 `canonicalize`。
- 工作区根目录**允许尚不存在**：校验时先找最近已存在的祖先并 `canonicalize`，创建后再次 `canonicalize`。
- 目标路径 = 已验证根目录 + 迭代号 + 安全单层目录名，**逐段 join**，不接受前端传来的完整路径作为写入目标。
- 已存在路径必须 `canonicalize` 并验证仍在预期根目录内（`starts_with` 于规范化路径）。
- 复制时拒绝符号链接与 reparse point（`platform::is_link_like`）。

### 4.3 前端不可信

前端提交的一切（路径、状态、可移除性、可否删 vendor、确认文本）后端每次都重新校验并重新计算。前端只显示后端返回的业务结论，不自行推导「可移除」「有效 worktree」「vendor 可删除」「clean」。

### 4.4 错误对象与脱敏（`error.rs`）

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")] Validation(String),   // code = "validation"
    #[error("已有操作执行中：{0}")] Busy(String),      // code = "busy"
    #[error("{0}")] NotFound(String),     // code = "notFound"
    #[error("{0}")] Conflict(String),     // code = "conflict"
    #[error("{0}")] ManifestDamaged(String), // code = "manifestDamaged"
    #[error("{0}")] Git(String),          // code = "git"
    #[error("{0}")] Io(String),           // code = "io"
    #[error("{0}")] Unsupported(String),  // code = "unsupported"
}
// serialize 为 { "code": "...", "message": "..." }
```

- `message` 截断到 600 字符（超出以 `…` 结尾）。
- 脱敏：对形如 `scheme://user:token@host` 的片段去掉 `user:token@`；对 `scheme://token@host` 去掉 `token@`。手写扫描实现，不引入 `regex`。
- 所有 Git 失败信息经 `git.rs` 统一走脱敏与截断后再抛出。

---

## 5. Git 调用规则（`git.rs`）

一律通过 `std::process::Command::new("git").args(&[...])` 调用，**禁止拼接 shell 命令字符串**。`git.rs` 对外只暴露有类型的函数，每个函数内部构造固定的参数向量；单元测试逐条比对 `Vec<String>`。

允许的子命令（白名单，超出即视为设计偏离）：

| 用途 | 参数向量 | 说明 |
| --- | --- | --- |
| 解析仓库根 | `rev-parse --show-toplevel` | 添加项目时 |
| 分支名校验 | `check-ref-format --branch <name>` | |
| 列举 remote | `remote` | |
| 远端分支（联网） | `ls-remote --heads <remote>` | 15 秒超时 + `GIT_TERMINAL_PROMPT=0` |
| 远端分支（本地缓存） | `for-each-ref --format=%(refname) refs/remotes` | |
| 拉取基分支 | `fetch <remote> <branch>` | 每项目一次 |
| 拉取目标分支 | `fetch --no-tags origin develop master` | 合并检查 |
| 解析 commit | `rev-parse --verify --quiet <ref>^{commit}` | `<ref>` 取 `FETCH_HEAD`、`refs/remotes/origin/<target>`、`refs/heads/<name>` 等；同时用于判定目标分支是否存在 |
| commit 摘要 | `log -1 --format=%h %s <hash>` | 组装 `unmergedCommits` 的 `"<短hash> <subject>"` |
| 本地分支存在 | `show-ref --verify --quiet refs/heads/<branch>` | |
| 创建 detached | `worktree add --detach <path> <commit>` | |
| 创建新分支 | `worktree add -b <branch> <path> <commit>` | |
| 列举 worktree | `worktree list --porcelain` | 完整解析不截断 |
| 移动 worktree | `worktree move [-f] <旧> <新>` | 脏工作区单个 `-f`；**禁止 `-f -f`** |
| 工作区状态 | `status --porcelain=v1 --untracked-files=all` | 完整解析不截断 |
| 提交是否被引用 | `for-each-ref --format=%(refname) --contains <commit> refs/remotes` | |
| 祖先判定 | `merge-base --is-ancestor <a> <b>` | |
| patch-id 对比 | `cherry <upstream> <head>` | |
| 三方树对比 | `merge-tree --write-tree <base> <head>` | |
| 树对象 | `rev-parse <ref>^{tree}` | 与 merge-tree 结果比较 |
| 本侧独有提交 | `rev-list --count <base>..HEAD` 与 `rev-list <base>..HEAD` | 005 的 hasChanges；`cherry` 的剩余 commit 列表 |
| 解析 HEAD | `rev-parse --verify HEAD`、`symbolic-ref -q --short HEAD` | 复核时 |
| 移除 worktree | `worktree remove [--force] <path>` | `--force` 仅 008 强制归档 |

超时：`ls-remote` 15 秒；其余不设超时（fetch 由用户网络决定，不实现取消）。所有 Git 子进程设置 `GIT_TERMINAL_PROMPT=0`，工作目录为目标仓库根。

`git.rs` 提供两条执行路径：`run(args)`（等待结束）与 `run_with_timeout(args, Duration)`（`spawn` + 轮询 `try_wait` + 超时 `kill`，超时返回 `AppError::Git("git ls-remote 超时（15 秒）")`）。`worktree move` 方法签名为 `worktree_move(old, new, force: bool)`，单测断言 `force=true` 时参数向量恰好含一个 `-f`。

所有子命令执行前先用 `GitArgs` 构造参数向量，构造函数与执行分离，便于单测逐条比对。

---

## 6. 持久化与并发

### 6.1 文件

- 全局配置：`platform::config_dir()/config.json`。写盘点：`save_config`；以及 `create_workspaces` 在校验通过后把迭代号写入 `recentIterations`（每批一次原子写）。
- 迭代清单：`{工作区根目录}/{迭代号}/.worktree-manager.json`。
- 所有 JSON 走**原子写入**（`atomic_json.rs`）：同目录唯一临时文件（`.worktree-manager.json.tmp-<pid>-<nanos>`）→ `write_all` → `flush` → `sync_all` → `rename` 覆盖；失败时删除临时文件。
- 损坏的清单**只报错**（`manifestHealth = damaged`），不覆盖、不重建、不删除。
- 不保存 remote URL、用户名、密码、Token、SSH 私钥、环境变量。

### 6.2 操作锁（`lib.rs::OperationState`）

```rust
pub struct OperationState { current: Mutex<Option<&'static str>> }
impl OperationState {
    pub fn acquire(&self, name: &'static str) -> Result<OperationGuard, AppError>  // 已占用 → Busy(当前操作名)
}
```

- 全局**单个非阻塞锁**，`try` 语义：已占用立即返回 `busy`，不排队、不取消。
- 必须持锁的命令与操作名（`busy` 消息用「已有操作执行中：<操作名>」）：

| 命令 | 操作名 |
| --- | --- |
| `create_workspaces` | 创建工作区 |
| `list_workspaces(reconcile=true)` | 复核状态 |
| `check_merge_status` | 检查合并 |
| `remove_worktree` / `remove_discovered_worktree` | 移除 worktree |
| `assess_archive` | 评估归档 |
| `archive_iteration` | 归档迭代 |
| `set_iteration_note` | 修改备注 |
| `set_iteration_hidden` | 隐藏迭代 |
| `reorder_project` | 调整顺序 |
| `move_project` | 移动项目 |

- 不持锁：`get_config`、`save_config`、`resolve_projects`、`list_workspaces(reconcile=false)`、`list_remote_branches`、`assess_removal`、`assess_discovered_removal`、`open_*`。
- 复核持锁的原因：011 可能回写清单。`assess_archive` 持锁的原因：它会对每个仓库执行 `fetch`，与 `check_merge_status` 并发会在同一仓库同时 fetch。

---

## 7. 接口契约

### 7.1 Tauri 命令（19 个）

全部在 `lib.rs` 的 `generate_handler!` 注册，并在 `src/api/tauri.ts` 用同名 camelCase 函数封装。参数名与 JSON 字段一律 camelCase（Rust 侧 `#[serde(rename_all = "camelCase")]`；Tauri 命令参数在 TS 侧同样用 camelCase）。

| 命令 | 入参 | 出参 | 锁 |
| --- | --- | --- | --- |
| `get_config` | — | `AppConfig` | 否 |
| `save_config` | `config: AppConfig` | 规范化后的 `AppConfig` | 否 |
| `resolve_projects` | `paths: string[]` | `ProjectResolution[]` | 否 |
| `list_workspaces` | `reconcile: boolean` | `WorkspaceGroup[]` | 仅 `reconcile=true` |
| `create_workspaces` | `request: CreateRequest` | `CreateBatchResult` | 是 |
| `list_remote_branches` | `projectId: string, includeRemote: boolean` | `RemoteBranches` | 否 |
| `check_merge_status` | `iteration: string, projectId?: string, worktreePath?: string` | `MergeCheckResult` | 是 |
| `assess_removal` | `iteration, projectId, worktreePath` | `RemovalAssessment` | 否 |
| `remove_worktree` | `request: RemoveRequest` | `null` | 是 |
| `assess_discovered_removal` | `iteration: string, worktreePath: string, sourcePath: string` | `RemovalAssessment` | 否 |
| `remove_discovered_worktree` | `iteration, projectId, worktreePath, sourcePath, confirmation, removeCopiedVendor` | `null` | 是 |
| `assess_archive` | `iteration` | `ArchiveAssessment` | 是（内部 fetch） |
| `archive_iteration` | `request: ArchiveRequest` | `ArchiveOutcome` | 是 |
| `set_iteration_note` | `iteration, note: string \| null` | `string \| null` | 是 |
| `set_iteration_hidden` | `iteration, hidden: boolean` | `string \| null`（`hiddenAt`） | 是 |
| `open_iteration` | `iteration` | `null` | 否 |
| `open_project` | `iteration, projectId, worktreePath` | `null` | 否 |
| `reorder_project` | `iteration, worktreePath, beforeWorktreePath: string \| null` | `null` | 是 |
| `move_project` | `request: MoveProjectRequest` | `MoveOutcome` | 是 |

前端封装名：`getConfig` `saveConfig` `resolveProjects` `listWorkspaces` `createWorkspaces` `listRemoteBranches` `checkMergeStatus` `assessRemoval` `removeWorktree` `assessDiscoveredRemoval` `removeDiscoveredWorktree` `assessArchive` `archiveIteration` `setIterationNote` `setIterationHidden` `openIteration` `openProject` `reorderProject` `moveProject`，以及事件订阅 `onCreateProgress` `onMergeCheckProgress` `onArchiveProgress`（返回 `() => void` 的 unlisten）。

### 7.2 事件（3 个）

| 事件 | 载荷 | 时机 |
| --- | --- | --- |
| `create-progress` | `CreateProgress { projectId, index, total, phase: queued\|fetching\|creating\|vendor\|completed\|failed, message }` | 创建逐项目阶段变化 |
| `merge-check-progress` | `MergeCheckProgress { iteration, projectId, worktreePath, index, total, phase: fetching\|checking\|record, message, record: MergeRecordResult \| null }` | 合并检查逐条；`phase=record` 时 `record` 非空 |
| `archive-progress` | 复用 `MergeCheckProgress`，`record` 恒为 `null` | 归档评估逐条 |

进度 `message` 要写**实际执行的命令**（如 `git fetch origin master`）。前端在组件卸载时必须 `unlisten()`。

### 7.3 浏览器演示模式（`api/tauri.ts`）

- 判定：`typeof window !== "undefined" && "__TAURI_INTERNALS__" in window` 为 Tauri 环境；否则为浏览器演示。
- 演示模式：读命令返回 `demo-data.ts` 的假数据；写命令与长任务返回 `Promise.reject({ code: "unsupported", message: "浏览器演示模式不支持此操作" })`；事件订阅返回空 unlisten。顶部横幅显示「浏览器演示模式：数据为只读假数据」。
- `@tauri-apps/api` 与 `@tauri-apps/plugin-dialog` 必须**动态 import**（在函数体内），保证 `test-frontend.mjs` 的 SSR 渲染与 `vite build` 都不会在 Node 侧触发 Tauri 全局对象访问。

---

## 8. 前端架构

- 无状态库：`App.vue` 持有页面状态 `page: 'workspaces' | 'create' | 'settings'`、`config`、`groups`、`toasts`、`banner`；通过 props / emits 与子组件通信；跨组件共享的只有 `provide/inject` 的 `toast()` 与 `setBanner()`。
- 合并检查结果 `Map<iteration, MergeCheckResult>` 只存内存（`WorkspaceList.vue` 内 `ref`），不落 localStorage。
- localStorage 键：`worktree-manager.visible-columns`、`worktree-manager.expanded-iterations`、`worktree-manager.hidden-zone-expanded`。
- 列定义、标签文案、颜色映射集中在 `utils/*.ts`，组件内不写死文案分支。
- 所有 `invoke` 失败统一转成 `{ code, message }`，页面级错误进顶部横幅，操作级错误进 toast（danger）。

---

## 9. 数据流概览

```text
设置页 ──save_config──▶ config.json
                            │
创建页 ──create_workspaces─▶ workspace.rs：公共目录 → 逐项目 fetch/add/vendor → 每项目后原子写清单
                            │                      └─ create-progress 事件 ─▶ OperationView
                            ▼
列表页 ◀─list_workspaces(false)── manifest.rs 快扫（枚举目录 + 读清单，不跑 Git）
列表页 ◀─list_workspaces(true)─── manifest.rs 复核（worktree list / HEAD / status / discovered）+ head_state.rs 改名回写
列表页 ◀─check_merge_status────── merge_check.rs（fetch → 三层判定）─ merge-check-progress ─▶ 逐行更新
移除   ──assess_removal / remove_worktree──▶ removal.rs（风险 → 确认文本 → worktree remove）
归档   ──assess_archive / archive_iteration─▶ archive.rs（复用 merge_check → 批量移除 → archivedAt）
排序/移动 ──reorder_project / move_project─▶ relocate.rs（数组重排 / worktree move + 两份清单）
```

工作区列表**不是第二份持久化状态**，而是「清单 + 实时核对结果」的投影。

---

## 10. 与原规格的偏离清单

| 编号 | 原规格 | 本设计 | 原因 |
| --- | --- | --- | --- |
| D1 | Windows 10/11 x64 免安装 EXE | 当前平台（macOS 优先）可执行文件，代码跨平台 | 用户决策 |
| D2 | 默认 `D:/Work/workspace` 与 `fd-common` 规则 | 默认为空、必填 | 用户决策 |
| D3 | `%APPDATA%/WorktreeManager/config.json` | `platform::config_dir()`，macOS 为 `~/Library/Application Support/WorktreeManager/config.json` | 平台适配 |
| D4 | `fd-common` 是保留标识 | 不再硬编码保留字，只禁止与已配置目标名冲突 | 默认规则已取消，保留字失去意义 |
| D5 | Explorer 打开目录 | `platform::open_in_file_manager`（macOS `open`） | 平台适配 |
| D6 | `windows-sys` 为必选依赖 | 改为 `cfg(windows)` 目标依赖；新增 `tauri-build` 构建依赖 | Tauri 工程必需 |
| D7 | 字体栈以 Segoe UI 开头 | 前置 `-apple-system, "PingFang SC"`，其余保留 | 平台适配 |
| D8 | 允许子命令表 | 补充 `rev-parse <ref>^{tree}`、`rev-list`、`symbolic-ref`、`rev-parse --verify --quiet <ref>^{commit}`（泛化）、`log -1 --format=%h %s <hash>` | 实现 005/007/复核与 commit 摘要所必需的最小补充 |
| D9 | `assess_archive` 不持锁 | 持锁 | 内部 fetch 与合并检查互斥 |

其余口径（13 条判定、接口契约、数据模型、枚举、UI token）与原规格一致。

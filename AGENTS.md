# AGENTS.md · 实现方项目约定

> 本文件面向实现本项目的 AI / 开发者。与 `docs/` 冲突时以 `docs/architecture.md`、`docs/data-model.md`、`docs/requirements.md` 为准。

## 1. 工作方式

- 先读 `docs/implementation-plan.md`，严格按 S1 → S9 串行推进；每步的「完成定义」未全部满足不得进入下一步。
- 每个功能开工前先读对应 `docs/design/NNN-*/design.md`。
- 一次只推进一个功能；不为「顺手」提前实现后续功能。
- 禁止虚报：报告的检查必须真实执行；不能执行的写「未执行 / 无法验证 + 原因」。不写「应该没问题」。
- 不擅自扩大范围：`docs/todo.md` 与 `docs/requirements.md §2.3` 的「不做」项发现了只记录，不实现。
- 每步结束按 `docs/implementation-plan.md §5` 格式汇报；每步必跑 `docs/implementation-plan.md §4` 的自检命令。
- 语言：代码注释、文档、UI 文案、错误信息全部中文；标识符、Git 术语、路径、命令保持英文。

## 2. 文档分工

- **当前态事实来源**：`docs/architecture.md`、`docs/data-model.md`、`docs/workflows.md`（`docs/ui-spec.md` 为视觉来源）。功能合入时必须同步更新对应章节。
- **历史决策记录**：`docs/design/NNN-<english-slug>/design.md`。只增不改：编号取当前最大值 +1，不复用；设计确认后**不回改正文**；实现偏离时在文末追加「实现记录」小节（做了什么、偏离了什么、为什么）。
- 新增设计文档至少含：背景与目标（现状/目标/非目标）、已确认决策（含被否决备选及原因）、技术方案概要（数据结构/命令/前端入口）、状态与枚举、已知局限。
- 枚举与 `models.rs` 逐个对照；新增字段先改 `docs/data-model.md` 再改代码。
- `docs/acceptance.md` 只在真实验证后打勾，格式见该文件开头。

## 3. 依赖白名单与版本锁定

- Rust 运行时依赖**仅**：`tauri = "2"`、`tauri-plugin-dialog = "2"`、`serde`（derive）、`serde_json`、`thiserror`、`chrono`（`clock`,`serde`）、`sha2`。
- `[target.'cfg(windows)'.dependencies]` 仅 `windows-sys`（`Win32_Foundation`,`Win32_Storage_FileSystem`）；`[build-dependencies]` 仅 `tauri-build`；`[dev-dependencies]` 仅 `tempfile`。
- 前端：`vue ^3.5`、`typescript ~5.9`、`vite ^7`、`@vitejs/plugin-vue ^6`、`vue-tsc ^3`、`@tauri-apps/api ^2`、`@tauri-apps/cli ^2`、`@tauri-apps/plugin-dialog ^2`。
- **禁止**：`tauri` 3.x（含 alpha）、`vite` 8.x、`typescript` 7.x；UI 框架、状态管理库、日期库、图标库、CSS 框架、测试框架、`dirs` / `open` / `regex` 等便利 crate。
- `package.json` scripts、`tsconfig.json`、Cargo release profile 见 `docs/architecture.md §1`。

## 4. 平台抽象

- 主验收平台 macOS；代码保持跨平台。
- 所有平台差异集中在 `src-tauri/src/platform.rs`（`config_dir` / `open_in_file_manager` / `is_link_like` / `paths_equal` / `is_absolute`）。**其它模块不得写 `cfg(windows)` / `cfg(unix)`**（`validation.rs` 的保留设备名规则是跨平台统一规则，不算平台分支）。
- 迭代号 / 目录名校验、大小写不敏感比较、`dragDropEnabled = false` 在全平台统一执行。

## 5. Git 调用规则

- 一律通过 `src-tauri/src/git.rs` 用 `std::process::Command::new("git").args(&[...])` 参数向量调用；**禁止拼接 shell 字符串**、禁止 `sh -c`。
- 只允许 `docs/architecture.md §5` 白名单里的子命令；新增子命令＝设计偏离，必须先补设计文档与白名单。
- 所有子进程设 `GIT_TERMINAL_PROMPT=0`；`ls-remote` 15 秒超时；工作目录为目标仓库根。
- 用户可见错误统一经 `error.rs` 截断（600 字符）与凭证脱敏。
- porcelain / refs / worktree list 输出**完整解析、不得截断**。
- `worktree move` 对脏工作区只用单个 `-f`；`locked` / `prunable` 由预检拦截；**禁止 `-f -f`**。
- `worktree remove --force` 仅 008 强制归档可用；移除流程不 stash、不 commit、不 push、不删分支。

## 6. 持久化

- 所有 JSON 走 `atomic_json.rs`：同目录临时文件 → `write_all` → `flush` → `sync_all` → `rename`；失败清理临时文件。
- 损坏的清单只报错（`manifestHealth = damaged`），不覆盖、不重建、不删除。
- 不保存 remote URL、用户名、密码、Token、SSH 私钥、环境变量。
- 配置文件不存在时返回默认值且不创建文件；用户保存后才写盘。

## 7. 安全

- 前端提交的一切（路径、状态、可移除性、确认文本、`removeCopiedVendor`）不可信，后端每次重新校验并重算。
- 前端只显示后端返回的业务结论，不自行推导「可移除」「有效」「clean」「vendor 可删」。
- 目标路径由已验证根目录 + 迭代号 + 安全单层目录名逐段 join；已存在路径 `canonicalize` 后 `ensure_within` 根目录。
- 复制拒绝符号链接与 reparse point；排除任意层级 `.git`。
- `capabilities/default.json` 只含 `core:default` 与 `dialog:allow-open`；CSP 见 `docs/architecture.md §4.1`；无 CDN、无内联脚本样式。

## 8. 新增 / 修改命令的四步

1. `models.rs`：新增请求 / 结果结构与枚举（`#[serde(rename_all = "camelCase")]`），并同步 `docs/data-model.md`。
2. `lib.rs`：写 Command 适配函数（只做反序列化 → 取锁 → 调服务 → 错误转换 → 发事件），加入 `generate_handler!`。
3. `src/types.ts`：新增对应 TS 类型（只含 `export type` / `export interface`）。
4. `src/api/tauri.ts`：新增同名 camelCase 封装，并在浏览器演示模式给出回退（读命令返回 `demo-data.ts` 假数据，写命令 reject `{ code: "unsupported" }`）。
- 同时更新 `docs/architecture.md §7` 命令表。

## 9. 操作锁

- 全局单个非阻塞锁 `OperationState`，`try` 语义：已占用立即返回 `busy`（message 含当前操作名）；不排队、不取消。
- **必须持锁**：`create_workspaces`、`remove_worktree`、`remove_discovered_worktree`、`assess_archive`、`archive_iteration`、`list_workspaces(reconcile=true)`、`check_merge_status`、`set_iteration_note`、`set_iteration_hidden`、`reorder_project`、`move_project`。
- **不持锁**：`get_config`、`save_config`、`resolve_projects`、`list_workspaces(reconcile=false)`、`list_remote_branches`、`assess_removal`、`assess_discovered_removal`、`open_*`。（`assess_archive` 持锁，因为它会 fetch。）
- Git 子进程在 `tauri::async_runtime::spawn_blocking` 中执行，命令为 `async`。

## 10. 事件

- 三个事件 `create-progress` / `merge-check-progress` / `archive-progress`，载荷见 `docs/data-model.md §4`。
- 进度 `message` 写实际执行的命令（如 `git fetch origin master`）。
- 前端订阅返回 unlisten 函数，组件 `onUnmounted` 必须调用。
- `@tauri-apps/api` 与 `@tauri-apps/plugin-dialog` 必须在函数体内动态 import。

## 11. 测试规则

- Rust：`cargo test`。`docs/implementation-plan.md §2` 各步列出的单测与集成测试是**必写**清单，不是建议。
- 集成测试用 `tempfile` + 本地裸仓库作 remote；本机无 `git` 时打印原因并 `return`，**生产代码不得因此跳过任何校验**。
- Git 参数向量测试逐条比对 `Vec<String>`；porcelain / status 解析测试覆盖正常 / `locked` / `prunable` / detached / 大量条目。
- 前端：只用 `scripts/test-frontend.mjs`（Vite `createServer` + `ssrLoadModule` + `createSSRApp` + `renderToString` + `node:assert/strict`），不引入任何测试框架；结束时 `server.close()` 并 `process.exit(0)`。
- 涉及拖拽、气泡、剪贴板的交互必须在真实 Tauri 窗口验证，浏览器预览不算。

## 12. 前端约定

- 无状态库；`App.vue` 持有页面状态，子组件用 props / emits；`provide/inject` 只用于 `toast()` 与 `setBanner()`。
- 文案、颜色映射集中在 `src/utils/*.ts`；组件内不写死文案分支。
- 合并检查结果只存内存；localStorage 只有 `docs/ui-spec.md §9` 的三个键。
- 视觉严格按 `docs/ui-spec.md`；图标只用内联 SVG 或文字符号。

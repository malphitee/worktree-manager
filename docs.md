# Worktree Manager · 单条自包含复刻提示词（一次性投喂版）

> 本文与 `00`～`09` 阶段包是**二选一的两种形态**：阶段包用于有文件系统、能分多轮续跑的 Codex 会话；本文用于**一次性投喂、单轮或少量轮次完成**的场合（无文件系统 / 无命令执行能力的环境也能用，降级规则见 §15）。
> 投喂前建议删除本节以上内容，只保留 `# 任务` 之后的部分。

---

# 任务

你要从零实现一个 Windows 桌面应用 **Worktree Manager**：面向 Windows 10/11 x64 的本地 Git worktree 桌面管理工具，用于按迭代（如 `7.3.0`）批量创建、复核、归档一批仓库的 worktree。

交付标准是「能编译、能运行、你亲自验证过的检查」，不是「写出看起来正确的代码」。以下全部内容是硬性规格，与你的直觉冲突时以本文为准。

---

## 1. 不可协商的工作方式

1. **先设计再实现**：每实现一个功能前，先给出该功能的设计说明（背景与目标 / 已确认决策 / 技术方案 / 状态与枚举 / 已知局限），再写代码。
2. **文档是产物的一部分**：环境支持写文件时，必须产出 §14 列出的文档；不支持时，在回复内给出等价的精简版（每个功能一段设计说明 + 最终验收结论）。
3. **禁止虚报**：报告任何检查结果时必须真实执行过；无法执行的明确写「未执行 / 无法验证」。不要写「应该没问题」。
4. **依赖白名单**：只允许 §3 列出的依赖。不引入 UI 框架、状态管理库、日期库、图标库、CSS 框架。
5. **语言**：代码注释、文档、UI 文案、错误信息全部中文（标识符与 Git 术语保持英文）。
6. **不擅自扩大范围**：§2.3 里「不做」的东西，发现邻近问题只记录，不实现。
7. **一次只推进一个功能**：不要为了「顺手」提前实现后续功能。

---

## 2. 产品定义

### 2.1 使用者与场景

一个开发者在一个迭代里要同时改动 10+ 个仓库（PHP / Go 混合）。他要把这些仓库的 worktree 统一落到 `{工作区根目录}/{迭代号}/{项目}` 下，把公共目录（如 `fd-common`）复制一份供所有项目共用，把 PHP 的 `vendor` 按 lock 一致性搬过来，然后在一个界面里复核每个 worktree 的真实状态、检查改动是否已合并进 `develop`/`master`，并在确认安全的前提下移除单个 worktree 或归档整个迭代。

### 2.2 术语表

| 术语                | 含义                                                         |
| ------------------- | ------------------------------------------------------------ |
| 迭代 / iteration    | 批次名，如 `7.3.0`、`sprint-42`；是工作区根目录下的单层目录名 |
| 工作区根目录        | 存放所有迭代目录的父目录，默认 `D:/Work/workspace`           |
| 公共目录规则        | 「源目录 → 迭代目标目录」映射，默认 `D:/Work/fd-common` → `{迭代目录}/fd-common` |
| 托管记录            | 迭代清单 `.worktree-manager.json` 里记录的 worktree          |
| discovered worktree | 磁盘上存在、但清单没有记录的 worktree                        |
| 基分支 baseRef      | 创建 worktree 时使用的基分支，归一化形如 `origin/release`    |
| 基准变动            | HEAD 相对**创建时记录的基分支**是否有本侧独有提交            |
| 快扫 / 复核         | 快扫＝只枚举目录 + 读清单，**不跑任何 Git**；复核＝`git worktree list` + HEAD/分支 + dirty + discovered 扫描 |

### 2.3 能力范围

**MVP 能力（11 条，全部必须实现）**

1. 配置 worktree 根目录、公共目录「源目录 → 目标目录」映射、人工选择的源项目。
2. 从本次 `git fetch <remote> <branch>` 解析出的不可变 Commit 串行创建多个 worktree。
3. 支持 Detached HEAD 或基于指定基分支创建全新本地分支。
4. 统一分支名与统一基分支同已选项目**单向**实时联动，逐项目仍可单独修改。
5. 按设置中的映射向迭代目录复制不含 `.git`、不跟随链接的公共目录快照；`fd-common` 只是默认配置，不是内建逻辑。
6. 添加项目时识别 PHP / Go / 其他项目，并显示 PHP 源仓库是否具备 `vendor`。
7. 创建时实时复核 PHP 项目，在两份 `composer.lock` 内容 Hash 相同时从源仓库复制 `vendor`。
8. 通过迭代清单复核并展示工作区真实状态（快扫 / 复核两档）。
9. 打开项目目录或迭代目录（Explorer）。
10. 在没有修改、未跟踪文件、独立提交或 worktree lock 风险时安全移除单个 worktree。
11. 全局配置与迭代清单均原子写入，不保存任何 Git 凭证。

**增强能力（14 项，全部必须实现）**

| 编号 | 能力                         | 一句话要求                                                   |
| ---- | ---------------------------- | ------------------------------------------------------------ |
| 001  | 迭代/分支合并状态检查        | 三层自动判定（merge / cherry / merge-tree）每条记录 × `origin/develop`、`origin/master` 的合并矩阵 |
| 002  | 复制路径与分支               | 列表里单击「Worktree 路径」「分支 / HEAD」两格复制到系统剪贴板，不新增后端命令 |
| 003  | 增量合并检查                 | 一次命令 + 事件流，逐条完成即推送，前端逐行就地更新          |
| 004  | 手动复核                     | 列表分「快扫 / 复核」两档，复核只由「复核状态」按钮触发      |
| 005  | 相对基分支的「有变更」       | 看 HEAD 相对记录创建时的基分支，不看创建时的 `baseCommit`    |
| 006  | 合并检查结果并入「基准变动」 | 复用逐条事件，不另跑复核                                     |
| 007  | 合并检查只查 active          | 与列表可见行一致                                             |
| 008  | 迭代归档                     | 评估 + 批量移除 + 写 `archivedAt`；干净记录不带 `--force`    |
| 009  | 迭代备注                     | 清单 `note`，≤50 字符、单行，空串＝清除                      |
| 010  | 迭代隐藏                     | 写 `hiddenAt`，移入页面底部「已隐藏迭代」收纳区，可恢复      |
| 011  | 分支改名同步                 | 清单分支名本地已不存在、worktree 仍注册同路径且检出别的分支 → 判「本地改名」，按 live 名给状态并原子回写清单 |
| 012  | 备注气泡                     | 悬浮显示全文，`Teleport` 到 body（卡片 `overflow: hidden` 会裁掉卡片内气泡） |
| 013  | 创建基分支                   | 创建页可选统一/逐项目基分支，`<datalist>` + 手输；远端列举本地优先、按需拉取 |
| 014  | 项目拖拽排序与跨迭代移动     | 迭代内拖拽重排 `projects` 数组；跨迭代用 `git worktree move` 真实搬运并迁移清单记录 |

**明确不做**：整迭代移除（008 归档不是删除，归档保留迭代目录与公共目录）、公共目录刷新/重新复制、自动扫描磁盘上的 Git 仓库、Git 凭证管理、`git pull`、`composer install`、打开 IDE/终端、分支自动删除、强制移除（008 的强制归档例外）、任务取消、定时刷新、自动更新、登录、云同步、团队功能。

---

## 3. 技术栈与硬约束

| 层       | 选型（锁定）                                                 |
| -------- | ------------------------------------------------------------ |
| 桌面框架 | Tauri v2，`bundle.active=false`，发布用 `tauri build --no-bundle` |
| 后端     | Rust edition 2021，`rust-version = 1.77.2`；crate 仅：`tauri`、`tauri-plugin-dialog`、`serde`、`serde_json`、`thiserror`、`chrono`（`clock`,`serde`）、`sha2`、`windows-sys`（`Win32_Foundation`,`Win32_Storage_FileSystem`）；dev-dep 仅 `tempfile` |
| 前端     | Vue 3.5（Composition API + `<script setup>`）+ TypeScript 5.9 + Vite 7 + `@vitejs/plugin-vue` + `vue-tsc`；`@tauri-apps/api`、`@tauri-apps/plugin-dialog` |
| 测试     | Rust `cargo test`；前端用 `scripts/test-frontend.mjs`（Vite `createServer` + `createSSRApp` + `renderToString` + Node `assert/strict`），**不引入测试框架** |

`package.json` scripts 固定：`dev: vite`、`test:frontend: node scripts/test-frontend.mjs`、`typecheck: vue-tsc --noEmit`、`build: vue-tsc --noEmit && vite build`、`tauri: tauri`。Vite 端口 `1420`，`clearScreen: false`。
`tsconfig.json`：`strict`、`noUnusedLocals`、`noUnusedParameters`、`verbatimModuleSyntax`、`moduleResolution: "bundler"`、`jsx: "preserve"`。
Cargo release profile：`lto = true`、`codegen-units = 1`、`opt-level = "s"`、`strip = true`、`panic = "abort"`。

### 3.1 安全与信任模型

- 不启动独立 HTTP 服务；不暴露通用 Shell 或任意文件系统接口；只启用 dialog 能力与白名单命令。
- CSP 严格为：`default-src 'self'; img-src 'self' asset: data:; style-src 'self'; script-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'`。脚本与样式全部随应用打包，无任何 CDN。
- 路径信任：配置里的仓库路径保存与使用前 `canonicalize`；工作区根目录**允许尚不存在**（先验证最近已存在的祖先，创建后再次 `canonicalize`）；目标路径由已验证根目录 + 迭代号 + 安全单层目录名逐段 join；已存在路径必须 `canonicalize` 并验证仍在预期根目录内；复制时拒绝符号链接与 Windows reparse point。
- 前端提交的一切（路径、状态、可移除性、可否删 vendor）**都不可信**，后端每次都重新校验。前端只显示后端返回的业务状态，不自行推导「可移除」「有效 worktree」「vendor 可删除」这类安全结论。

### 3.2 Git 调用规则

一律通过 `src-tauri/src/git.rs` 用**参数向量**调用 `std::process::Command`，**禁止拼接 shell 命令字符串**。允许的子命令仅限：

`rev-parse --show-toplevel`、`check-ref-format --branch <name>`、`remote`、`ls-remote --heads <remote>`（15 秒超时 + `GIT_TERMINAL_PROMPT=0`）、`for-each-ref refs/remotes`、`fetch <remote> <branch>`、`fetch --no-tags origin develop master`、`rev-parse --verify FETCH_HEAD^{commit}`、`show-ref --verify --quiet refs/heads/<branch>`、`worktree add --detach <path> <commit>`、`worktree add -b <branch> <path> <commit>`、`worktree list --porcelain`、`worktree move [-f] <旧> <新>`、`status --porcelain=v1 --untracked-files=all`、`for-each-ref --contains <commit>`、`merge-base --is-ancestor`、`cherry`、`merge-tree --write-tree`、`worktree remove [--force] <path>`。

- 用户可见的错误信息要限制长度并对敏感 URL 凭证脱敏（把 `https://user:token@host` 中的 `user:token` 去掉）。
- 安全判定需要的 porcelain / refs / worktree list 输出必须**完整解析、不得截断**（截断会导致误判）。
- `worktree move` 对脏工作区只用**单个** `-f`；`locked` 由上层预检拦掉，**不使用 `-f -f`**。

### 3.3 持久化与并发

- 全局配置：`%APPDATA%/WorktreeManager/config.json`（不存在时返回默认配置，只有用户保存后才创建文件）。
- 迭代清单：`{工作区根目录}/{迭代号}/.worktree-manager.json`。
- 所有 JSON 走**原子写入**：写同目录唯一临时文件 → `flush` → `sync_all` → `rename` 覆盖；失败清理临时文件。损坏的清单**只报错，不覆盖、不重建、不删除**。
- 不保存 remote URL、用户名、密码、Token、SSH 私钥、完整环境变量。
- 全局**单个非阻塞操作锁**：创建、移除、归档、复核（`reconcile=true`）、合并检查、备注、隐藏、排序、跨迭代移动都必须先获取，已占用立即返回「已有操作执行中」。**不实现取消**。
- 复核要持锁的原因：它可能在检测到分支重命名时回写清单。

### 3.4 Windows 特化（踩过的坑，必须照做）

- 窗口：标题 `Worktree Manager`、1440×900、最小 1080×700、可缩放、居中。
- **`app.windows[].dragDropEnabled = false`**：Windows/WebView2 下系统文件拖放由 OLE DropTarget 实现，会抢走页内 HTML5 拖拽（014 的排序与跨迭代移动），表现为光标变「禁止」、拖影不跟随。本应用不使用系统文件拖入，关闭无副作用。
- 迭代号校验必须拒绝：空值、`.`、`..`、路径分隔符、非法字符（`\ / : * ? " < > |`）、控制字符、Windows 保留设备名（`CON`/`PRN`/`AUX`/`NUL`/`COM1-9`/`LPT1-9`…）、末尾空格或句点。
- 项目标识不能与公共目录目标名冲突；`fd-common` 是保留标识。
- 文件系统比较一律大小写不敏感。

---

## 4. 目录与模块职责

```text
src-tauri/
  tauri.conf.json   窗口 + CSP + bundle.active=false
  Cargo.toml        release profile 同 §3
  src/
    main.rs         仅调用 lib 的 run()
    lib.rs          Tauri 装配、Command 适配层、generate_handler!、OperationState
    models.rs       全部序列化结构与枚举
    error.rs        AppError → 稳定错误对象（含长度限制与凭证脱敏）
    config.rs       配置定位、旧格式迁移、校验、normalize_and_save、项目解析
    validation.rs   迭代号、项目标识、目录名、分支、绝对路径校验
    path_utils.rs   canonicalize、允许缺失的解析、边界验证、逐段 join
    atomic_json.rs  原子 JSON 读写
    git.rs          Git 子命令封装（参数数组）
    copy.rs         公共目录快照复制（排除 .git、不跟随链接、临时目录 + rename）
    vendor.rs       composer.lock SHA-256 判定与 vendor 复制
    workspace.rs    串行创建编排 + 进度事件
    manifest.rs     清单读写、状态投影、discovered 扫描、快扫/复核
    head_state.rs   分支 live 状态与「本地改名」判定（011）
    merge_check.rs  三层合并判定（001）
    archive.rs      归档评估与批量移除（008）
    relocate.rs     跨迭代移动（014）
    removal.rs      风险检查与安全移除编排
src/
  main.ts / App.vue  应用壳：页面状态机（workspaces | create | settings）、侧栏、顶部错误横幅、toast、模态层挂载点
  styles.css         从静态页面提取的 CSS token 与基础样式（见 §8）
  types.ts           与 Rust 结构一一对应的 TS 类型
  api/tauri.ts       全部 invoke 与 listen 的唯一出口（浏览器构建下提供只读演示回退）
  components/AppSidebar.vue, WorkspaceList.vue, CreateWorkspace.vue, SettingsView.vue,
             OperationView.vue, RemovalDialog.vue, MergeDetailDialog.vue, ArchiveDialog.vue
  utils/base-ref.ts, config.ts, merge.ts, note.ts, status.ts
scripts/test-frontend.mjs
```

Command 适配层只做「反序列化参数 → 调服务 → 错误转换 → 长任务发射进度事件」，不写业务规则。

---

## 5. 接口契约

### 5.1 Tauri 命令（19 个，全部在 `lib.rs` 的 `generate_handler!` 注册，并在 `src/api/tauri.ts` 封装）

| 命令                         | 入参                                                         | 出参                   |
| ---------------------------- | ------------------------------------------------------------ | ---------------------- |
| `get_config`                 | —                                                            | `AppConfig`            |
| `save_config`                | `config`                                                     | 规范化后的 `AppConfig` |
| `resolve_projects`           | `paths: string[]`                                            | `ProjectResolution[]`  |
| `list_workspaces`            | `reconcile: boolean`                                         | `WorkspaceGroup[]`     |
| `create_workspaces`          | `request: CreateRequest`                                     | `CreateBatchResult`    |
| `list_remote_branches`       | `projectId`, `includeRemote`                                 | `RemoteBranches`       |
| `check_merge_status`         | `iteration`, 可选 `projectId`/`worktreePath`                 | `MergeCheckResult`     |
| `assess_removal`             | `iteration`, `projectId`, `worktreePath`                     | `RemovalAssessment`    |
| `remove_worktree`            | `request: RemoveRequest`                                     | `()`                   |
| `assess_discovered_removal`  | `sourcePath`                                                 | `RemovalAssessment`    |
| `remove_discovered_worktree` | `iteration`, `projectId`, `worktreePath`, `sourcePath`, `confirmation`, `removeCopiedVendor` | `()`                   |
| `assess_archive`             | `iteration`                                                  | `ArchiveAssessment`    |
| `archive_iteration`          | `request: ArchiveRequest`                                    | `ArchiveOutcome`       |
| `set_iteration_note`         | `iteration`, `note: string \| null`                          | `string \| null`       |
| `set_iteration_hidden`       | `iteration`, `hidden: boolean`                               | `string \| null`       |
| `open_iteration`             | `iteration`                                                  | `()`                   |
| `open_project`               | `iteration`, `projectId`, `worktreePath`                     | `()`                   |
| `reorder_project`            | `iteration`, `worktreePath`, `beforeWorktreePath: string \| null` | `()`                   |
| `move_project`               | `request: MoveProjectRequest`                                | `MoveOutcome`          |

前端封装名：`getConfig` `saveConfig` `resolveProjects` `listWorkspaces` `createWorkspaces` `listRemoteBranches` `checkMergeStatus` `assessRemoval` `removeWorktree` `assessDiscoveredRemoval` `removeDiscoveredWorktree` `assessArchive` `archiveIteration` `setIterationNote` `setIterationHidden` `openIteration` `openProject` `reorderProject` `moveProject` `onCreateProgress` `onMergeCheckProgress` `onArchiveProgress`。

### 5.2 事件（3 个）

| 事件                   | 载荷                                                         | 时机                                            |
| ---------------------- | ------------------------------------------------------------ | ----------------------------------------------- |
| `create-progress`      | `{ projectId, index, total, phase: queued\|fetching\|creating\|vendor\|completed\|failed, message }` | 创建逐项目阶段                                  |
| `merge-check-progress` | `{ iteration, projectId, worktreePath, index, total, phase: fetching\|checking\|record, message, record? }` | 合并检查逐条；`phase=record` 带刚完成的那条结果 |
| `archive-progress`     | 复用 `MergeCheckProgress` 结构，`record` 恒为 `null`         | 归档评估逐条                                    |

进度文案要显示**实际执行的命令**（如 `git fetch origin master`）。前端必须在组件卸载时 `unlisten()`。

---

## 6. 数据模型（JSON 用 `camelCase`，模型带 `schemaVersion`，值为 `1`）

### 6.1 `%APPDATA%/WorktreeManager/config.json`

```json
{
  "schemaVersion": 1,
  "workspaceRoot": "D:/Work/workspace",
  "sharedDirectories": [
    { "sourcePath": "D:/Work/fd-common", "targetDirectory": "fd-common" }
  ],
  "projects": [
    { "id": "api3", "repositoryPath": "D:/Work/php-project/api3", "projectType": "php", "vendorAvailable": true }
  ],
  "recentIterations": ["7.3.0"]
}
```

约束：`targetDirectory` 大小写不敏感唯一、安全单层目录名；`sourcePath` 是 Windows 绝对路径，创建前要求可读普通目录；`projects[].id` 非空、单层目录安全、大小写不敏感唯一、不能与公共目录目标冲突；`projectType ∈ php|go|other|unknown`；`vendorAvailable` 是添加/保存时的本地快照（创建时重新检查真实目录）；`repositoryPath` 是已存在的真实 Git 根目录；`recentIterations` 是创建页的迭代号历史候选。
**旧格式迁移**：`fdCommonSource` → 一条目标为 `fd-common` 的规则；`goCommonSource` 与旧格式中 `enabled=false` 的规则直接丢弃。

### 6.2 `{工作区根目录}/{迭代号}/.worktree-manager.json`

```json
{
  "schemaVersion": 1,
  "iteration": "7.3.0",
  "createdAt": "2026-07-30T10:00:00Z",
  "note": null,
  "hiddenAt": null,
  "archivedAt": null,
  "sharedDirectories": [
    { "ruleId": "fd-common", "sourcePath": "D:/Work/fd-common", "targetDirectory": "fd-common",
      "targetPath": "D:/Work/workspace/7.3.0/fd-common", "status": "copied", "message": null }
  ],
  "projects": [
    { "projectId": "api3", "sourceRepository": "D:/Work/php-project/api3",
      "worktreePath": "D:/Work/workspace/7.3.0/api3", "headMode": "branch",
      "branch": "feature/example", "baseCommit": "0123456789abcdef", "baseRef": "origin/release",
      "createdAt": "2026-07-30T10:01:00Z", "lifecycle": "active",
      "createResult": { "status": "created", "message": null },
      "vendor": { "sourcePath": "D:/Work/php-project/api3/vendor", "lockHash": "sha256-hex",
                  "status": "copied", "copiedByTool": true, "message": null },
      "postSteps": [ { "name": "vendor", "status": "success", "message": null } ],
      "removedAt": null }
  ]
}
```

约束：同一迭代以规范化后的 `worktreePath` 唯一；同一 `projectId` 允许多条（目录名为 `projectId`、`projectId-2`、`projectId-3`…）；`projects` **数组顺序就是列表展示顺序**；失败项目允许目录尚未创建、`baseCommit` 为空，但必须保存预期 `worktreePath`、请求与错误原因。

### 6.3 枚举总表

| 字段                         | 取值                                                         |
| ---------------------------- | ------------------------------------------------------------ |
| `sharedDirectories[].status` | `pending \| reused \| copied \| failed`（旧清单的 `notRequired` 仍可读） |
| `headMode`                   | `branch \| detached`                                         |
| `lifecycle`                  | `active \| removed \| createFailed`                          |
| `createResult.status`        | `created \| alreadyExists \| failed`                         |
| `vendor.status`              | `copied \| notPhp \| sourceMissing \| lockMissing \| lockMismatch \| targetExists \| copyFailed` |
| `postSteps[].status`         | `success \| skipped \| failed`                               |
| `validity`（投影）           | `valid \| missingDirectory \| notRegistered \| headMismatch \| sourceMissing \| removed \| discovered \| unknown` |
| `manifestHealth`             | `valid \| missing \| damaged`                                |
| `MergeCellStatus`            | `merged \| contained \| unmerged \| targetMissing \| branchMissing \| notCheckable \| error`（附加标志 `stale`、`dirty` 可并存） |
| `RemovalRisk.severity`       | `warning \| blocking`；`code` 至少含 `trackedChanges` `untrackedFiles` `unpushedCommits` `detachedCommits` `worktreeLocked` `pathInvalid` `manifestInvalid` |

### 6.4 投影与内存结构

```text
WorkspaceGroup  iteration, iterationPath, manifestHealth, manifestMessage, openable,
                note, hiddenAt, sharedDirectories[], projects[]
WorkspaceProject projectId, branchDisplay, baseCommit, baseRef, sourceRepository, worktreePath,
                vendorStatus, createdAt, validity, openable, removable, renamedFrom, dirty, hasChanges
MergeCheckResult iteration, checkedAt, records[]{ projectId, branchDisplay, worktreePath, lifecycle,
                 develop, master: MergeCellResult{status,unmergedCommits,errorMessage,stale,dirty},
                 hasChanges?, dirty? }
RemovalAssessment iteration, projectId, worktreePath, confirmationText, allowed,
                 vendorOnlyCleanupAvailable, risks[]{code,severity,message,paths[]}
ArchiveAssessment iteration, confirmationText(=迭代号), checkedAt, clean,
                 records[]{projectId,branchDisplay,worktreePath,develop,master,dirty,clean,blockers[]}
ArchiveOutcome   iteration, removedCount, failed[]{projectId,worktreePath,message}, archived
MoveOutcome      iteration, targetIteration, projectId, worktreePath, targetWorktreePath, targetDirectory
```

工作区列表**不是第二份持久化状态**，而是「清单 + 实时核对结果」的投影。

---

## 7. 判定口径（13 条，写死在实现里）

1. **创建串行、失败隔离**：多项目按 UI 顺序串行；单项目失败不影响后续项目，也不回滚已成功项目；每个项目处理完立即持久化清单。
2. **fetch → 不可变 Commit**：每项目执行一次 `fetch <remote> <branch>`，立即把本次 `FETCH_HEAD` 解析为不可变 Commit Hash，Detached 与新分支都直接从该 Hash 创建；清单同时写归一化 `baseRef`。**不使用**「最新 master」这类会漂移的引用。
3. **公共目录先行**：在任何 Git 操作前按配置顺序完成全部公共目录；任一失败 → 整批终止、清单记 `failed`、不开始 fetch。目标已存在且由清单管理则复用（不覆盖）；未被清单管理的同名目录或活动 worktree 阻止创建。复制排除任意层级 `.git`，不跟随符号链接/Junction，先复制到同父目录临时目录再原子 rename 落位。
4. **vendor 六条件**：新 worktree 有 `composer.json`、源仓库有 `vendor`、两端都有 `composer.lock`、两份 lock 的 SHA-256 完全一致、目标没有 `vendor`。不满足是可解释的**跳过**而非创建失败；复制失败保留 worktree 并写清单。**不执行 `composer install`**。
5. **快扫 vs 复核**：启动、保存设置、创建/移除/归档后只做快扫（不跑 Git，存在的 worktree 标 `unknown`＝「未复核」）；完整复核只由「复核状态」按钮触发，复核中保留旧列表、完成后整体替换。不提供刷新按钮与定时轮询。
6. **基准变动**：`hasChanges` ＝ HEAD 相对「创建时记录的基分支」有本侧独有提交。解析链：`baseRef` → 去掉 remote 前缀的本地名 → `origin/master` → `master`，取第一个可解析者。discovered 行无清单记录，直接用 `origin/master`（否则 `master`）。`dirty` ＝ 有未提交改动。列表优先用合并检查给出的 `hasChanges`/`dirty`。
7. **合并三层判定**（前者命中即止）：① `merge-base --is-ancestor <branch> origin/<target>` → `merged`；② `git cherry origin/<target> <branch>` 的 patch-id 全部被包含 → `merged`；③ `merge-tree --write-tree origin/<target> <branch>` 的结果树与目标分支树相同 → `contained`（疑似 squash）；都不通过 → `unmerged` 并附剩余 commit 列表。目标分支固定 `origin/develop` + `origin/master`，不配置化；仓库缺该分支 → 该格 `targetMissing`，不整体失败。单仓库 fetch 失败 → 降级用本地 remote-tracking 缓存并给该仓库全部结果标 `stale`。对比基于 fetch 后的远端跟踪引用，不用本地 `develop`/`master`。**只查 `lifecycle=active`**。
8. **移除风险**：实时检查 tracked 修改、untracked 文件、未被远端引用的提交、Detached 独立提交、worktree lock；任一风险默认阻止，**不提供 `--force`、不 stash、不 commit、不 push、不删分支**。唯一 untracked 内容是**工具复制的 vendor** 时，可在输入 `{迭代号}/{目录名}` 二次确认后先删 vendor 再执行不带 `--force` 的 `worktree remove`。执行前重新计算风险并校验确认文本严格相等。成功后保留本地分支、迭代目录、所有公共目录与清单历史。
9. **归档 clean 判定**：develop 与 master 单元格均为 `merged | contained | targetMissing`、`dirty = false`、本次 fetch 未失败（`stale = false`）。`clean` 与 `blockers` 由后端判定，前端不推导。归档**全部成功才写 `archivedAt`**；单条失败不中断整批。
10. **优先级**：`archivedAt` 非空 → 迭代**完全不出现**在列表里（`list_groups` 直接跳过，快扫与复核一致）；`hiddenAt` 非空 → 仍投影，但由前端移入页面底部「已隐藏迭代」收纳区。归档保留本地分支、迭代目录与公共目录。
11. **本地改名判定**：清单里的分支名在源仓库已不存在、而 worktree 仍注册在同一路径并检出了另一个分支 → `validity = valid`、`branchDisplay` 取 live 名、`renamedFrom` 记下清单旧名（界面显示「⟳ 已同步改名」），并把清单 `branch` **原子回写**为 live 名（只改这一字段）；清单分支仍存在但与 live 不一致（切分支/串台）仍判 `headMismatch` 且清单不动。合并检查与安全移除同样按 live 名判定。
12. **基分支归一化**：空串＝`origin/master`；`<remote>/<branch>` 首段与已配置 remote 同名时按该 remote 解析；裸分支名优先 `origin`，没有 `origin` 且只有一个 remote 时用该 remote；remote 不存在或分支名非法 → 该项目失败。`createFailed` 记录同样保留归一化结果。
13. **排序与跨迭代移动**：落点语义统一为「插到锚点行之前，锚点为 `null` 即追加到该迭代末尾」，前端只提交锚点、不做索引换算；同迭代 drop 走 `reorder_project`，跨迭代走 `move_project`。目标目录名沿用 `projectId`/`projectId-N`，**被目标迭代既有记录占用就顺延——包括 `removed`/`createFailed` 这类仍留在清单里的非活动记录**，否则会撞出重复 `worktreePath` 使清单读成损坏。移动先 `git worktree move` 搬运本体，成功后再写两份清单（先目标、后源，各自原子写）。

---

## 8. 视觉与交互规范

若环境提供静态设计稿（`code.html` + `screen.png` + `DESIGN.md`），按其提取 token 并保留在 `stitch_design_system_generator/`（作为视觉来源、**不参与构建**）；没有则直接使用下表。

```css
:root {
  font-family: "Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
  color: #1f2328; background: #f5f7f9; font-synthesis: none;
  --primary: #2b6cb0; --primary-hover: #245b96; --primary-pressed: #1e4b7c; --primary-tint: #e7f0f8;
  --surface: #fff; --sidebar: #eff2f5; --canvas: #f5f7f9; --hover: #e9edf1;
  --border: #d5dce3; --divider: #e6eaee; --secondary: #59636e; --muted: #66717d;
  --success: #237a57; --success-tint: #e8f4ee;
  --warning: #8a5a00; --warning-tint: #fff3d6;
  --danger: #b42318; --danger-tint: #fdecea;
  --mono: "Cascadia Mono", "Cascadia Code", Consolas, monospace;
}
```

- 侧栏 216px 固定；内容区 `overflow: auto`；页面内边距 `22px 24px 84px`；设置页 `max-width: 1300px`。
- 字号：页面标题 22px/650，卡片标题 17px，表头 12px/650，正文 13px；`code`/路径/Commit 用 `--mono` 12.5px。
- 表格：表头高 36px、行高 50px、单元格 `max-width: 230px`、路径单行省略；行 hover `#fbfcfd`；分支色 `#155f92` 且允许 `break-all`；窄窗口用 `.table-scroll { overflow-x: auto }`。
- 按钮高 34px、圆角 6px、600 字重；图标按钮 30px 方形，hover `--primary-tint`，危险态 `--danger-tint`。
- 标签 `.tag`：neutral `#eef1f4`/`--secondary`、success、warning、danger，高 22px、圆角 6px、12px 字。
- 焦点态统一 `outline: 2px solid rgba(43,108,176,.42)`；禁用态 `opacity: .5` + `not-allowed`。
- 图标一律内联 SVG 或文字符号（`▤` `✎` `⟳` `⠿`），不引入图标库。
- 必须包含 `@media (prefers-reduced-motion: reduce)` 全局降级，以及 `max-width: 1220px` 的栅格降级。
- 模态层：backdrop `rgba(22,27,32,.38)`，面板 `min(720px, 90vw)` / `max-height: 86vh`，页脚背景 `#f8fafb`。toast 固定右上角，三态着色。全局加载失败用顶部 `app-banner`。
- 侧栏两项导航「工作区」「设置」；创建页底部固定操作条（`position: fixed; right: 0; bottom: 0; left: 216px`）。

**列表列定义（顺序固定，可勾选显隐）**：`project`（恒显）、`branch`、`dirty`（表头「基准变动」）、`mergeDevelop`（表头 `develop`）、`mergeMaster`（表头 `master`）、`baseCommit`、`source`、`worktreePath`、`vendor`、`createdAt`、`status`（恒显）、`actions`（恒显）。
**localStorage 键**：`worktree-manager.visible-columns`（默认全可见）、`worktree-manager.expanded-iterations`、`worktree-manager.hidden-zone-expanded`（`"1"`/`"0"`）。

**关键交互**：迭代卡片头部有折叠箭头、迭代号、备注标记 `▤`、路径、以及「✎ 备注 / 隐藏 / 归档 / 打开目录 / 检查合并」按钮组；「已隐藏迭代」收纳区在页面底部，默认折叠、无隐藏项时整区不渲染、不受搜索影响、不渲染项目明细表；创建页有「最近迭代」胶囊标签（点击填入，可 `×` 移除）；单击路径/分支格即复制并短暂显示「已复制」。

---

## 9. 实现顺序（10 步，每步做完再进下一步）

**第 0 步 · 文档基线**：产出 §14 的文档集（或回复内等价精简版），并给出第一份功能设计说明（`001-merge-check`）。

**第 1 步 · 工程骨架与视觉还原**：建立 Vite + Vue 3 + TS + Tauri 工程（配置同 §3）；三个页面（工作区列表 / 创建工作区 / 设置）、三个模态层（移除确认 / 合并详情 / 归档）、全局 toast 与顶部横幅全部按 §8 静态还原，用真实中文文案与假数据；无任何 CDN；此时命令集可为空。
自检：`npm run typecheck`、`npm run test:frontend`、`npm run build`、`cargo check`、`tauri build -- --no-bundle`。

**第 2 步 · Rust 领域能力**：实现 `error.rs`、`models.rs`、`validation.rs`、`path_utils.rs`、`atomic_json.rs`、`config.rs`、`git.rs`、`copy.rs`、`vendor.rs`，以及 `manifest.rs` 的读写与投影纯部分（discovered 扫描留到第 3 步）。
必写单元测试：迭代号校验（`7.3.0`/`sprint-42` 通过，`..`/`a/b`/`a:b`/`CON`/`x.`/`x `/空串/控制字符拒绝）；项目标识大小写不敏感唯一 + 保留名 + 与公共目录冲突；配置迁移；路径越界与「允许缺失」；原子写与损坏文件不被改写；copy 的四类用例（含 `.git` 目录/文件、符号链接、目标已存在、中途失败不留半成品）；vendor 六条件全覆盖；Git 参数构造逐条比对 `Vec<String>`；porcelain 解析（正常 / `locked` / `prunable` / detached）；快扫投影的 `manifestHealth`×`validity` 组合；`next_directory_name` 在占用 `removed` 记录时仍分配 `-2`。

**第 3 步 · 创建与移除闭环（MVP 可运行）**：实现 `workspace.rs`（按 §7.1/7.2/7.3/7.4 的顺序编排 + `create-progress`）、`removal.rs`、清单完整复核与 discovered 扫描、`lib.rs` 的 `OperationState` 与 MVP 命令注册（`get_config` `save_config` `resolve_projects` `list_workspaces` `create_workspaces` `assess_removal` `remove_worktree` `assess_discovered_removal` `remove_discovered_worktree` `open_iteration` `open_project`），并接通前端真实调用（浏览器构建下用只读演示数据 + 顶部提示「浏览器演示模式」）。
集成测试（`tempfile` + 本地裸 remote）：`FETCH_HEAD` 解析的 commit 与 `origin/master` 一致且 worktree HEAD 相等；两项目中第二个远端分支不存在 → 部分失败且清单两条都在；detached 独立提交阻止移除；普通 untracked 文件阻止移除、删除后可移除；同名本地分支导致该项目失败且不建目录；连续创建三个目录为 `p`/`p-2`/`p-3`、同项目同分支二次创建为 `alreadyExists`；公共目录失败时无项目进入 fetch；每次项目处理后清单都是完整 JSON。

**第 4 步 · MVP 验收与便携发布**：逐条验证 §11 的 1～5 节；`bundle.active=false` 产出免安装 EXE；把 EXE 单独复制到**空目录**启动并确认主页面加载；写 `README.md`（能力清单、环境要求、常用命令、`--no-bundle` 含义、数据位置、迁移说明、文档索引）；把不属于 MVP 的问题写进 `docs/todo.md`。

**第 5 步 · 001～007（合并检查族）**：按 §7.7 实现 `merge_check.rs` 与 `check_merge_status`；`MergeCheckProgress` 逐条推送（003）；结果作为内存快照带 `checkedAt`，不落盘；「有变更」修正为相对 `baseRef`（005）；`MergeRecordResult` 带 `hasChanges`/`dirty` 供列表就地更新（006）；只查 active（007）；002 是纯前端剪贴板（单击整格即复制，失败降级 `execCommand`，不新增命令、不占锁）；004 把 `list_workspaces` 明确分成快扫/复核两档且复核持锁。
测试：真 merge / rebase / squash 三种 fixture 各命中一层；fetch 失败降级 `stale`；缺目标分支 → `targetMissing`；操作锁互斥；detached / 分支缺失路径。
已知局限（照实写进设计文档，不要试图修掉）：squash 后又改同一批文件 → 层 3 误报；squash 冲突解决改写内容 → 可能误报；合并后被 revert → 显示未完全包含；对比对象是分支 tip，未提交改动不在任何分支中；本地分支已删的记录无法判断。005 的局限：本地基分支落后于远端时显示「有变更」（刻意取舍，另有合并检查负责远端真相）。

**第 6 步 · 008～010**：按 §7.9 实现 `archive.rs` 的两个命令与 `archive-progress`；`ArchiveDialog.vue` 按干净/强制两态；`list_groups` 跳过 `archivedAt` 非空迭代。实现 `note`（去首尾空白 ≤50 字符、禁换行、空串＝清除；创建请求 `note` **缺省＝不改动**，不要用 `note: ""` 表达不改动）与 `hiddenAt`（纯展示层，`false` 恢复；前端收纳区规则见 §8；在已隐藏迭代上继续创建会自动取消隐藏）。三者都持操作锁。
已知局限：归档不删分支/目录/公共目录；备注不支持富文本与换行；隐藏只有时间戳、无审计信息。

**第 7 步 · 011～012**：实现 `head_state.rs` 的改名判定与清单原子回写（只改 `branch`），复核完成 toast「已同步 N 个分支重命名」（N=0 不提示），下游统一按 live 名判定。备注气泡用 `<Teleport to="body">` + `position: fixed`（坐标由 `getBoundingClientRect()` 算出），`window` 上 `capture: true` 监听 `scroll` 收起，组件卸载时移除监听，触发元素与气泡留 4～8px 间隙避免闪烁。
已知局限：改名后若又在源仓库建了同名新分支会判成 `headMismatch`（无法区分「改名」与「切分支」）；`renamedFrom` 只在本次复核内提示，不落盘；气泡纯展示、不可选中复制。

**第 8 步 · 013～014**：实现 `resolve_base_ref` 归一化（§7.12）与 `list_remote_branches`（`includeRemote=false` 只读 `refs/remotes`、不联网；`true` 走 `ls-remote`，15 秒超时 + `GIT_TERMINAL_PROMPT=0`，整页触发时**串行**，失败只降级候选并把原始 git/ssh 报错放 tooltip）；列表显示「基于 XXX」。实现 `reorder_project` 与 `move_project`（§7.13 与下面的预检）；拖拽手柄「⠿」是唯一拖拽起点（`<tr>` 不设 `draggable`），搜索过滤时手柄置灰，落点是自己原位时完全不发请求，成功后清掉两个迭代的合并检查结论并重新加载列表。
`move_project` 预检（任一不通过即报错且不改动任何内容）：两个迭代清单都可读；源记录 `lifecycle=active`；目录名符合 `projectId`/`projectId-N`；worktree 目录存在且在该源仓库的 `worktree list` 中注册；未被 `worktree lock` 锁定；`prunable` 拒绝（不得用 `-f -f` 绕过）。搬运成功后先写目标清单（锚点处插入、`worktreePath` 改新路径、其余快照字段不变）再删源记录，各自原子写；不引入跨文件事务，失败时错误信息给出「点复核状态或手工修正清单」的恢复指引。
已知局限：第二次写入失败会出现清单与磁盘不一致（只给恢复指引，不自动回滚）；已归档迭代不能作为落点；排序只影响展示与检查顺序。
**改完必须在真实 Tauri 窗口里验证拖拽**（浏览器预览覆盖不到 WebView2 的拖放行为）；若拖拽时光标变「禁止」、拖影不跟随，先检查 `dragDropEnabled` 是否为 `false`。

**第 9 步 · 全量最终验收**：逐条验证 §11 全部条目，执行 §10 全部命令，做 §13 的文档一致性审计，输出 §12 的交付物清单。

---

## 10. 构建与自检命令

```powershell
npm install
npm run typecheck
npm run test:frontend
npm run build
cd src-tauri; cargo test; cd ..
npm run tauri build -- --no-bundle
# 便携验证：复制到空目录并启动
$dst = "$env:TEMP\wm-portable-check"; New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item src-tauri\target\release\worktree-manager.exe $dst -Force
Start-Process (Join-Path $dst "worktree-manager.exe")
```

`bundle.active=false` + `targets=[]` 保证不生成 MSI/NSIS。集成测试需要 Git；本机没有 Git 时测试应 `skip` 并打印原因，而**不是**让生产代码跳过校验。

---

## 11. 验收清单（逐条验证，未验证不许打勾）

**配置**

- [ ] 首次启动展示默认工作区路径和一条可编辑的 `fd-common` 规则，不要求已有配置文件。
- [ ] 可多选目录并解析到真实 Git 根目录；普通目录给出明确错误。
- [ ] 公共目录目标名称大小写不敏感唯一；项目标识大小写不敏感唯一、`fd-common` 被拒、不能与公共目录目标冲突。
- [ ] 保存后配置是有效 UTF-8 JSON 且无凭证数据；旧 `fdCommonSource` 迁移成功，`goCommonSource`/`enabled=false` 被丢弃。
- [ ] 从列表移除项目不影响磁盘仓库。

**创建**

- [ ] 迭代号接受 `7.3.0`/`sprint-42`，拒绝路径穿越、非法字符、设备保留名、尾部空格/句点。
- [ ] 分支名由 Git 校验；留空创建 Detached HEAD；已存在分支只使该项目失败。
- [ ] 统一分支名与统一基分支单向实时联动，新选项目继承当前值，单项编辑不反向影响统一值。
- [ ] 每个项目恰好 fetch 一次自己的基分支，worktree HEAD 等于 `FETCH_HEAD` 解析出的 Commit，清单记录归一化 `baseRef`。
- [ ] 基分支留空＝`origin/master`；`origin/release`、`upstream/xxx`、裸分支名都可用；打开创建页不发起任何远端连接；拉取失败降级为手输且不阻断创建。
- [ ] 按 UI 顺序串行，部分失败后继续；有效现有 worktree 显示「已存在」（去重键＝项目＋分支，不含基分支）；非管理目录冲突阻止创建。
- [ ] 同迭代重复创建同名项目按 `-2`/`-3` 顺延，且把 `removed`/`createFailed` 也算作占用。

**公共目录与 vendor**

- [ ] 新迭代先完成全部公共目录，失败时清单记 `failed` 且无项目进入 fetch/create。
- [ ] 快照不含任意 `.git` 文件/目录，不跟随符号链接或 Junction；已存在的规则目标目录被复用。
- [ ] 未管理同名目录与 worktree 冲突阻止创建；`go-common` 不作为内建强制项。
- [ ] 识别 PHP/Go/其他与 PHP `vendor` 状态；仅 lock Hash 相同时复制 vendor；Go 显示「无需复制」；lock 不匹配与复制失败都保留 worktree。

**清单与列表**

- [ ] 写入采用临时文件 + 原子替换；启动只做快扫（标记「未复核」），复核需点按钮；无刷新按钮与轮询。
- [ ] 清单缺失/损坏显示异常且不改目录，不可写备注/隐藏/归档，不能作为拖拽落点。
- [ ] 创建/移除后列表立即更新；可打开存在的目录（不存在时按钮禁用）；复核能发现 discovered worktree（快扫不出现）。
- [ ] 列显隐、迭代折叠、隐藏区展开态重启后保持。

**安全移除**

- [ ] tracked 修改、普通 untracked、未被远端引用提交、Detached 独立提交、lock 五类风险都阻止移除。
- [ ] 不执行 force、stash、commit、push、分支删除。
- [ ] 仅工具复制的 vendor 为唯一 untracked 内容时可确认后删除 vendor 并正常移除；确认文本严格等于 `{iteration}/{目录名}`。
- [ ] 成功后保留分支、迭代目录、所有公共目录与更新后的清单。

**合并检查与归档（001/003/006/007/008）**

- [ ] 真 merge / rebase / squash 都能被三层判定识别；缺目标分支显示「目标分支不存在」而不整体失败；只查 active。
- [ ] 结果逐条推送，列表某条完成即更新；单仓库 fetch 失败降级为「数据可能过时」并继续。
- [ ] 「基准变动」在 rebase 或基分支前进后不误报，且优先采用合并检查结果。
- [ ] 归档 clean 判定＝单元格合格 + 不脏 + 不 stale；不干净且未勾选强制归档时整体拒绝。
- [ ] 干净记录不带 `--force`，强制归档才带；全部成功才写 `archivedAt`；写后该迭代不再出现；归档保留分支/目录/公共目录；单条失败不中断整批。

**备注 · 隐藏 · 改名（009/010/011/012）**

- [ ] 备注 ≤50 字符、禁换行、空串清除；创建请求缺省不改动已有备注；无备注无任何常驻痕迹，有备注显示 `▤`。
- [ ] 气泡 `Teleport` 到 body，折叠迭代与已滚动页面下都不被裁切，滚动自动收起。
- [ ] 隐藏只写 `hiddenAt`；移入底部收纳区、默认折叠、无隐藏项不渲染、不受搜索影响；恢复回主列表；继续创建自动取消隐藏。
- [ ] `archivedAt` 优先于 `hiddenAt`（归档迭代两个区域都不出现）。
- [ ] 改名后复核：状态仍「有效」、显示 live 名与「已同步改名」、清单只回写 `branch`；清单分支仍存在但与 live 不一致时判 `headMismatch` 且清单不动；改名后在合并检查与移除中不再报「分支不存在」。

**基分支与拖拽（013/014）**

- [ ] 进入创建页不发起远端连接；`⟳` 失败时降级候选并给提示，不阻断创建；归一化三态正确，`createFailed` 保留归一化结果；列表显示「基于 XXX」。
- [ ] 手柄是唯一拖拽起点，点击复制路径/分支仍可用，文本可选择，搜索过滤时手柄置灰。
- [ ] 落点语义为「插到锚点之前」（`null`＝末尾）；拖到原位不发请求；排序与移动持久化到清单数组顺序，重启后保持。
- [ ] 跨迭代移动真实搬运本体，两份清单都可读、`worktreePath` 不重复；移动后两个迭代的合并检查结论被清掉；`locked`/`prunable` 无法移动。

**安全与并发**

- [ ] 前端无通用 Shell/文件系统权限，后端只暴露白名单命令；路径经 `canonicalize` 与边界验证；Git 用参数数组；CSP 不允许远程资源且无 CDN。
- [ ] 任意两个写操作/长任务互斥，第二个立即被拒；用户可见 Git 错误经过长度限制与凭证脱敏。

**构建与发布**

- [ ] `cargo test`、`npm run typecheck`、`npm run test:frontend`、`npm run build`、`tauri build -- --no-bundle` 全部通过且不生成 MSI/NSIS。
- [ ] 单独复制到空目录的 EXE 可启动并加载主页面。

---

## 12. 交付物清单（最终回复必须包含）

```text
代码：src-tauri/src/*.rs（模块数与行数）、src/**（组件数与行数）、scripts/test-frontend.mjs
文档：README.md、AGENTS.md、docs/{requirements,architecture,workflows,data-model,implementation-plan,acceptance,todo}.md、
      docs/design/001..014/design.md
产物：src-tauri/target/release/worktree-manager.exe（路径、体积、构建时间）
验证：cargo test / typecheck / test:frontend / build / 空目录启动 的结果摘要
```

## 13. 文档一致性规则（防漂移）

- `docs/architecture.md`、`docs/data-model.md`、`docs/workflows.md` 是**系统当前态的单一事实来源**，功能合入时必须同步更新对应章节。
- `docs/design/NNN-<english-slug>/design.md` 是**当次决策的历史记录**：编号取当前最大值 +1，只增不复用；设计确认后**不回改正文**，实现偏离时新增「实现记录」小节说明。
- 每个设计文档至少含：背景与目标（现状/目标/非目标）、已确认决策（含被否决的备选及原因）、技术方案概要（数据结构/命令/前端入口）、状态与枚举、已知局限。
- `AGENTS.md` 必须写明：设计文档规则、全局文档分工、Git 调用规则、原子写入规则、新增命令的注册与前端封装要求、操作锁互斥要求。
- 最终审计输出格式：`<文件> §<章节>：<发现的问题> → <修正方式>`，无问题的写「无偏差」。枚举必须与 `models.rs` 逐个对照。

## 14. 文档产物（环境无文件系统时降级为回复内精简版）

`README.md`（能力清单、环境要求、常用命令、`--no-bundle` 含义、数据位置与迁移、文档索引）、`AGENTS.md`、`docs/requirements.md`、`docs/architecture.md`、`docs/workflows.md`（12 条流程：首次启动与设置 / 创建工作区 / 启动列表与手动复核 / 打开目录 / 复制路径与分支 / 合并状态检查 / 安全移除 / 归档迭代 / 隐藏与恢复迭代 / 迭代备注 / 调整顺序与跨迭代移动 / 失败处理）、`docs/data-model.md`、`docs/implementation-plan.md`、`docs/acceptance.md`（用 §11）、`docs/todo.md`（§2.3 的不做清单，并注明「MVP 验收后如需规划整迭代移除，必须单独定义跨项目风险汇总、部分移除失败恢复和公共目录/清单保留策略，不能简单递归删除目录」）、`docs/design/001..014/design.md`。

## 15. 环境降级规则

- **有文件系统 + 能执行命令**：按 §9 顺序逐功能实现，每步真实执行 §10 并报告结果。
- **有文件系统但只能给出补丁/片段**：仍按 §9 顺序输出完整文件内容（不要用省略号或「其余同上」占位），并明确列出你需要我执行哪些命令来验证。
- **无文件系统（纯对话模型）**：按 §9 顺序一次性输出全部代码，每个文件用代码块标注路径；§11 的验收降级为「人工验证清单」，在结尾明确写明「以下检查我无法执行」，逐条给出对应的手工验证步骤。
- **无命令执行能力**：不得声称任何构建或测试通过；改为列出「需要你执行的命令清单」与「每条命令的预期输出」。

## 16. 输出与汇报格式

```text
## 完成的功能
- <编号/名称>：<一句话>（新增/修改文件：<路径>）
## 我实际执行的命令
- <命令> → <结果：通过/失败/未执行（原因）>
## 与设计的偏离
- <无 / 具体偏离 + 原因 + 写入了哪个设计文档的哪一节>
## 未验证项
- <条目 + 原因 + 请用户如何验证>
## 下一步
- <下一步编号与内容>
```

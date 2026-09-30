# Worktree Manager

面向单个开发者的本地桌面工具：按迭代号（如 `7.3.0`）把多个 Git 仓库的 worktree 统一创建到 `{工作区根目录}/{迭代号}/{项目}` 下，复制公共目录快照与 PHP `vendor`，并在一个界面里复核状态、检查合并、安全移除、归档整个迭代。

技术栈：Tauri v2 + Rust · Vue 3.5 + TypeScript 5.9 + Vite 7。不联网（除 `git fetch` / `git ls-remote`）、不登录、不同步。

## 能力清单

**MVP（11 条）**

1. 配置 worktree 根目录、公共目录「源目录 → 目标目录」映射、人工选择的源项目。
2. 从本次 `git fetch <remote> <branch>` 解析出的不可变 Commit 串行创建多个 worktree。
3. 支持 Detached HEAD 或基于指定基分支创建全新本地分支。
4. 统一分支名与统一基分支同已选项目单向实时联动，逐项目仍可单独修改。
5. 按设置中的映射向迭代目录复制不含 `.git`、不跟随链接的公共目录快照。
6. 添加项目时识别 PHP / Go / 其他项目，并显示 PHP 源仓库是否具备 `vendor`。
7. 创建时在两份 `composer.lock` 内容 Hash 相同时从源仓库复制 `vendor`。
8. 通过迭代清单复核并展示工作区真实状态（快扫 / 复核两档）。
9. 用系统文件管理器打开项目目录或迭代目录。
10. 在没有修改、未跟踪文件、独立提交或 worktree lock 风险时安全移除单个 worktree。
11. 全局配置与迭代清单均原子写入，不保存任何 Git 凭证。

**增强（14 项）**

- 001 迭代/分支合并状态检查：三层判定（merge / cherry / merge-tree）× `origin/develop`、`origin/master`
- 002 单击复制 Worktree 路径与分支名
- 003 增量合并检查：事件流逐条推送，前端逐行更新
- 004 手动复核：快扫 / 复核两档，复核只由按钮触发
- 005 「基准变动」相对创建时记录的基分支判定
- 006 合并检查结果并入列表「基准变动」列
- 007 合并检查只查 `active` 记录
- 008 迭代归档：评估 + 批量移除 + `archivedAt`
- 009 迭代备注（≤50 字符、单行）
- 010 迭代隐藏与恢复（页面底部收纳区）
- 011 分支改名同步：按 live 名判定并回写清单
- 012 备注气泡（Teleport 到 body）
- 013 创建基分支：统一 / 逐项目基分支，远端列举按需拉取
- 014 项目拖拽排序与跨迭代移动（`git worktree move`）

明确不做的事项见 `docs/todo.md`。

## 环境要求

- macOS（本轮验收平台；代码保持跨平台，Windows / Linux 未验证）
- Rust ≥ 1.77.2（`rustup` 安装的 stable 即可）
- Node ≥ 20（含 npm）
- git（集成测试与运行时都需要）
- Xcode Command Line Tools（`xcode-select --install`）

## 常用命令

```bash
npm install
npm run dev                          # Vite 开发服务器（端口 1420，浏览器演示模式）
npm run tauri dev                    # 真实 Tauri 窗口开发
npm run typecheck                    # vue-tsc --noEmit
npm run test:frontend                # node scripts/test-frontend.mjs
npm run build                        # vue-tsc --noEmit && vite build
(cd src-tauri && cargo test)         # Rust 单测 + 集成测试
npm run tauri build -- --no-bundle   # 发布构建
# 便携验证（macOS）
dst="$(mktemp -d)/wm-portable-check"; mkdir -p "$dst"
cp src-tauri/target/release/worktree-manager "$dst/"
"$dst/worktree-manager" &
```

## `--no-bundle` 的含义

`tauri.conf.json` 里 `bundle.active = false`、`bundle.targets = []`，构建**只产出裸可执行文件**，不生成 `.app` / `.dmg`（Windows 上不生成 MSI / NSIS）。

产物路径：`src-tauri/target/release/worktree-manager`（macOS Mach-O；Windows 上为 `worktree-manager.exe`）。该文件可单独复制到任意目录直接启动。

## 数据位置

| 数据 | 路径 |
| --- | --- |
| 全局配置 | macOS：`~/Library/Application Support/WorktreeManager/config.json`；Windows：`%APPDATA%\WorktreeManager\config.json`；Linux：`$XDG_CONFIG_HOME/WorktreeManager/config.json`（缺省 `~/.config/...`） |
| 迭代清单 | `{工作区根目录}/{迭代号}/.worktree-manager.json` |

- 配置文件不存在时使用默认值（工作区根目录为空、无公共目录规则），**首次启动需在设置页填写工作区根目录**；只有点「保存」后才创建文件。
- 旧配置迁移：顶层 `fdCommonSource` 自动迁移为一条目标为 `fd-common` 的公共目录规则；`goCommonSource` 与 `enabled=false` 的规则被丢弃。迁移在内存中进行，保存后写回新格式。
- 不保存任何 Git 凭证、remote URL 或环境变量。

## 文档索引

| 文档 | 内容 |
| --- | --- |
| `AGENTS.md` | 实现方项目约定：文档分工、依赖白名单、Git 调用、原子写、操作锁、测试规则 |
| `docs/requirements.md` | 使用者与场景、能力范围、13 条判定口径、非功能约束 |
| `docs/architecture.md` | 总体架构：技术栈锁定、平台策略、模块职责、安全模型、Git 白名单、持久化与锁、接口契约 |
| `docs/data-model.md` | 配置与清单 JSON、枚举总表、请求 / 结果 / 投影结构、TS 类型规则 |
| `docs/workflows.md` | 12 条用户流程的端到端步骤 |
| `docs/ui-spec.md` | CSS token、布局、表格列定义、按钮标签、模态、页面交互、文案对照、localStorage 键 |
| `docs/implementation-plan.md` | S0-S9 实施步骤、依赖图、可并行项、自检命令、汇报格式、交付物清单 |
| `docs/acceptance.md` | 逐条验收清单与验证方法 |
| `docs/todo.md` | 明确不做的事项与实现期间发现的问题 |
| `docs/design/001-merge-check/design.md` | 合并状态三层判定 |
| `docs/design/002-copy-path-and-branch/design.md` | 单击复制路径与分支 |
| `docs/design/003-incremental-merge-check/design.md` | 增量合并检查事件流 |
| `docs/design/004-manual-reconcile/design.md` | 快扫 / 复核两档 |
| `docs/design/005-changes-relative-to-base-ref/design.md` | 相对基分支的「基准变动」 |
| `docs/design/006-merge-result-into-list/design.md` | 合并检查结果并入列表 |
| `docs/design/007-merge-check-active-only/design.md` | 合并检查只查 active |
| `docs/design/008-archive-iteration/design.md` | 迭代归档 |
| `docs/design/009-iteration-note/design.md` | 迭代备注 |
| `docs/design/010-iteration-hidden/design.md` | 迭代隐藏 |
| `docs/design/011-branch-rename-sync/design.md` | 分支改名同步 |
| `docs/design/012-note-tooltip/design.md` | 备注气泡 |
| `docs/design/013-create-base-ref/design.md` | 创建基分支 |
| `docs/design/014-drag-reorder-and-move/design.md` | 拖拽排序与跨迭代移动 |

## 实现状态

> 最后更新：S4（MVP 验收与便携构建）。S5–S9 完成后在本节继续追加。

**已完成步骤**

| 步骤 | 内容 | 状态 |
| --- | --- | --- |
| S1 | 工程骨架 + 视觉静态还原（13 个组件、19 个命令封装、SSR 测试） | 完成 |
| S2 | Rust 领域层：`error` / `models` / `platform` / `validation` / `path_utils` / `atomic_json` / `config` / `git` / `copy` / `vendor` / `manifest`（纯部分） | 完成 |
| S3 | 创建与移除闭环：`workspace.rs` 串行编排、`removal.rs` 风险评估、完整复核 + discovered 扫描、11 个 MVP 命令、前端接通 | 完成（真实窗口手工验证需人工执行） |
| S4 | MVP 验收（`docs/acceptance.md` §1–§5）+ 便携构建 + README 实测更新 | 完成 |
| S5 | 001–007：`merge_check.rs` 三层判定 + `merge-check-progress` 事件流；002 剪贴板复制；004 手动复核分档；005/006/007 判定与列表合并 | 完成 |
| S6 | 008–010：`archive.rs` 归档评估与批量移除；`set_iteration_note` / `set_iteration_hidden`；前端归档对话框、备注内联编辑、隐藏收纳区 | 完成 |
| S7 | 011–012：复核识别本地改名并原子回写清单；`NoteTooltip`（标记 + Teleport 气泡、hover/focus、capture 监听收起） | 完成 |
| S8 | 013–014：`base_ref::list_remote_branches` 与创建页候选 datalist / `⟳`；`relocate.rs` 排序与跨迭代移动 + 前端拖拽 | 完成（真实窗口拖拽验证需人工执行） |
| S9 | 全量验收（`acceptance.md` §6–§11 标注）+ 文档一致性审计 + 交付物清单 | 完成 |

**产物**

| 项 | 值 |
| --- | --- |
| 路径 | `src-tauri/target/release/worktree-manager` |
| 类型 / 体积 | `Mach-O 64-bit executable arm64` / 约 3.6 MB |
| 构建 | `npm run tauri build -- --no-bundle`，release（`lto` + `opt-level=s` + `strip` + `panic=abort`），增量约 40 s |

**自检结果（最近一次真实执行）**

| 命令 | 结果 |
| --- | --- |
| `npm run typecheck` | 通过 |
| `npm run test:frontend` | 通过（30 项断言） |
| `npm run build` | 通过（JS ≈ 130 kB / CSS ≈ 12 kB） |
| `(cd src-tauri && cargo test)` | 通过（97 单测 + 50 集成测试） |
| `(cd src-tauri && cargo clippy --all-targets)` | 0 warning / 0 error |
| `npm run tauri build -- --no-bundle` | 通过 |
| 便携验证 | 可执行文件复制到空临时目录后启动，进程持续运行、无标准错误输出 |

**未验证项**

- 真实 Tauri 窗口的人工交互：窗口标题 / 尺寸 / 居中、目录选择对话框、系统文件管理器打开、剪贴板、**拖拽（含拖影与落点）**、备注气泡的滚动收起与翻转、内联编辑键盘操作、复核期间列表不闪空。相关验收条目已在 `docs/acceptance.md` 中标注原因（均需人工在窗口内执行）。
- Windows / Linux 平台行为：本轮验收平台为 macOS；相关分支代码保留但未编译验证（`cargo check --target x86_64-pc-windows-msvc` 因本机未安装该 target 未执行）。
- `relocate`「第二次清单写入失败」的恢复指引文案：需要构造「目标可写、源不可写」的只读状态，本机以普通用户难以稳定复现，属代码审查结论。


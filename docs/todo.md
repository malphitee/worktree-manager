# Worktree Manager · 待办与不做清单

> 本文记录**明确不做**的能力、实现期间发现但不在范围内的问题、以及非本轮验收平台的待验证项。发现邻近问题时**只记录、不实现**（`implementation-plan.md §0` 第 5 条）。

---

## 1. 明确不做（来自 `requirements.md §2.3`）

| 能力 | 为什么不做 | 将来要做的前提 |
| --- | --- | --- |
| 整迭代移除（删除） | 008 归档不是删除：归档保留迭代目录、公共目录与清单；一键递归删除会把跨项目的未推送提交、未提交改动、公共目录一起抹掉，风险不可解释 | **MVP 验收后如需规划整迭代移除，必须单独定义跨项目风险汇总、部分移除失败恢复和公共目录/清单保留策略，不能简单递归删除目录** |
| 公共目录刷新 / 重新复制 | 公共目录是创建时的快照；刷新意味着覆盖用户可能已修改的目录，需要 diff 与冲突策略 | 定义「快照来源版本」记录与覆盖前的差异确认流程 |
| 自动扫描磁盘上的 Git 仓库 | 项目由用户人工添加；自动扫描会引入误识别与大目录遍历成本 | 明确扫描根、深度上限、排除规则与用户确认步骤 |
| Git 凭证管理 | 工具不保存任何凭证（`architecture.md §6.1`）；凭证由系统 credential helper / ssh-agent 负责 | 不计划 |
| `git pull` | 创建基于 `fetch` + 不可变 commit；`pull` 会引入 merge/rebase 副作用 | 不计划 |
| `composer install` | vendor 只按 lock 一致性复制；执行安装需要网络、PHP 环境与超时/取消机制 | 定义 PHP 运行环境探测、超时与输出流展示 |
| 打开 IDE / 终端 | 只提供系统文件管理器打开（`platform::open_in_file_manager`）；IDE 路径因人而异 | 定义可配置的外部命令白名单与参数校验 |
| 分支自动删除 | 移除与归档均保留本地分支（判定口径 8、9）；删分支不可逆 | 定义「分支已合并且无本地独有提交」的独立确认流程 |
| 强制移除单个 worktree | 判定口径 8：任一风险即阻止，不提供 `--force`；仅 008 强制归档例外 | 不计划（用户可在源仓库手工处理） |
| 任务取消 | 操作锁为非阻塞单锁、无取消（`architecture.md §6.2`）；中途取消 `fetch`/`worktree add` 会留下半成品 | 定义每个阶段的可中断点与清理策略 |
| 定时刷新 / 轮询 | 判定口径 5：只有快扫与手动复核两档；轮询会持续跑 Git 并与操作锁冲突 | 不计划 |
| 自动更新 | 便携单文件产物，无更新通道 | 不计划 |
| 登录 / 云同步 / 团队功能 | 单机单人工具，不联网（除 `git fetch` / `ls-remote`） | 不计划 |

---

## 2. 实现期间发现（实现方填写）

> 规则：发现即记录；不实现；注明所属步骤（S1–S9）与功能编号。处理列固定填「记录不实现」，除非用户明确要求纳入。

| 日期 | 发现 | 所属步骤 / 功能 | 处理 |
| --- | --- | --- | --- |
| | | | 记录不实现 |

---

## 3. 非本轮验收平台的待验证项

本轮验收平台为 macOS（`architecture.md §2.1`）。以下分支代码保留、`cargo check` 保证可编译，但**真实行为未验证**：

| 平台 | 待验证项 | 涉及代码 |
| --- | --- | --- |
| Windows | `FILE_ATTRIBUTE_REPARSE_POINT` 检测（Junction / 符号链接）在复制与路径校验中的真实行为 | `platform::is_link_like`（`windows-sys`） |
| Windows | `%APPDATA%\WorktreeManager\config.json` 定位与首次保存时目录创建 | `platform::config_dir` |
| Windows | `explorer <path>` 打开目录；路径含空格、盘符、UNC 时的行为 | `platform::open_in_file_manager` |
| Windows | 保留设备名（`CON`、`con.txt`、`COM1` 等）、末尾空格/句点在 NTFS 上的真实拒绝行为与错误信息 | `validation.rs`（规则本身跨平台执行，此处验证的是与真实文件系统的一致性） |
| Windows | WebView2 下 `dragDropEnabled=false` 是否消除 HTML5 拖拽被 OLE DropTarget 抢占（光标「禁止」、拖影不跟随） | `tauri.conf.json`、`WorkspaceList.vue` 拖拽 |
| Windows | `git worktree move` 跨盘符 / 路径大小写差异时的 `canonicalize` 结果 | `relocate.rs`、`path_utils.rs` |
| Windows | `cargo check --target x86_64-pc-windows-msvc`（本机未安装该 target 时记「未执行」） | 全部 |
| Linux | `$XDG_CONFIG_HOME` / `~/.config/WorktreeManager` 定位 | `platform::config_dir` |
| Linux | `xdg-open <path>` 在无桌面环境时的失败信息 | `platform::open_in_file_manager` |
| Linux | 大小写敏感文件系统（ext4）下「大小写不敏感比较」与真实目录冲突的差异（`A` 与 `a` 可同时存在，但工具视为冲突） | `platform::paths_equal`、`next_directory_name` |
| Linux | WebKitGTK 下的 HTML5 拖拽行为 | `WorkspaceList.vue` |

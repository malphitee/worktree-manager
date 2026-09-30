# Worktree Manager · 视觉与交互规范

> 实现方按本文静态还原页面。没有设计稿文件，本文表格即视觉来源。禁止引入 UI 框架、CSS 框架、图标库。

---

## 1. CSS Token（`src/styles.css` 的 `:root`）

```css
:root {
  font-family: -apple-system, "SF Pro Text", "PingFang SC", "Segoe UI Variable", "Segoe UI",
               "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
  color: #1f2328;
  background: #f5f7f9;
  font-synthesis: none;
  -webkit-font-smoothing: antialiased;

  --primary: #2b6cb0; --primary-hover: #245b96; --primary-pressed: #1e4b7c; --primary-tint: #e7f0f8;
  --surface: #fff; --sidebar: #eff2f5; --canvas: #f5f7f9; --hover: #e9edf1;
  --border: #d5dce3; --divider: #e6eaee; --secondary: #59636e; --muted: #66717d;
  --success: #237a57; --success-tint: #e8f4ee;
  --warning: #8a5a00; --warning-tint: #fff3d6;
  --danger: #b42318; --danger-tint: #fdecea;
  --branch: #155f92;
  --row-hover: #fbfcfd;
  --mono: "SF Mono", Menlo, "Cascadia Mono", "Cascadia Code", Consolas, monospace;
  --sidebar-width: 216px;
}
```

必须包含：

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: .01ms !important; transition-duration: .01ms !important; }
}
@media (max-width: 1220px) {
  .settings-grid, .create-grid { grid-template-columns: 1fr; }
}
```

---

## 2. 布局

| 元素 | 规格 |
| --- | --- |
| 应用壳 | `display: grid; grid-template-columns: var(--sidebar-width) 1fr; height: 100vh` |
| 侧栏 | 宽 216px 固定，背景 `--sidebar`，右侧 1px `--border`；两项导航「工作区」「设置」，当前项背景 `--primary-tint`、文字 `--primary` |
| 内容区 | `overflow: auto`；页面内边距 `22px 24px 84px` |
| 设置页 | `max-width: 1300px` |
| 创建页底部操作条 | `position: fixed; right: 0; bottom: 0; left: 216px; height: 64px; background: var(--surface); border-top: 1px solid var(--border)`；右对齐「返回」「开始创建」 |
| 顶部横幅 `.app-banner` | 位于内容区顶部，全宽，`--danger-tint` 背景（错误）或 `--warning-tint`（演示模式），含关闭按钮 |
| toast | `position: fixed; top: 16px; right: 16px`，垂直堆叠，三态（success / warning / danger）着色，4 秒自动消失，hover 暂停 |

---

## 3. 字号与文本

| 用途 | 规格 |
| --- | --- |
| 页面标题 | 22px / 650 |
| 卡片标题 | 17px / 600 |
| 表头 | 12px / 650，`--secondary`，大写不转换 |
| 正文 | 13px |
| `code` / 路径 / Commit | `--mono` 12.5px |
| 辅助说明 | 12px，`--muted` |

---

## 4. 表格

- 表头高 36px，行高 50px；单元格 `max-width: 230px`；路径单行省略（`white-space: nowrap; overflow: hidden; text-overflow: ellipsis`）并在 `title` 上放全文。
- 行 hover 背景 `--row-hover`。
- 分支列颜色 `--branch`，允许 `word-break: break-all`。
- 外层 `.table-scroll { overflow-x: auto }` 供窄窗口横向滚动。
- 可复制格（`worktreePath`、`branch`）：`cursor: copy`；点击后该格内短暂（1.2 秒）显示「已复制」小标签，替换而非叠加原文。
- 拖拽手柄列固定在 `project` 列最左，字符 `⠿`，`cursor: grab`；搜索过滤中置灰 `opacity: .35; cursor: not-allowed`。

### 4.1 列定义（顺序固定；可勾选显隐）

| key | 表头 | 恒显 | 内容 |
| --- | --- | --- | --- |
| `project` | 项目 | 是 | 手柄 + `projectId`；`renamedFrom` 非空时附「⟳ 已同步改名」标签 |
| `branch` | 分支 / HEAD | | `branchDisplay`，可复制；下方小字「基于 {baseRef}」 |
| `dirty` | 基准变动 | | `hasChanges` → 「有变更」warning / 「无」neutral；`dirty` 为 true 附「未提交」danger；`null` 显示「未复核」 |
| `mergeDevelop` | develop | | `MergeCellStatus` 标签，可点开合并详情；`stale` 附「可能过时」 |
| `mergeMaster` | master | | 同上 |
| `baseCommit` | 基准 Commit | | 短 hash（7 位），`title` 全文 |
| `source` | 源仓库 | | 路径省略显示 |
| `worktreePath` | Worktree 路径 | | 路径省略显示，可复制 |
| `vendor` | vendor | | `VendorStatus` 标签；`notPhp` 显示「无需复制」 |
| `createdAt` | 创建时间 | | 本地时间 `YYYY-MM-DD HH:mm` |
| `status` | 状态 | 是 | `Validity` 标签 |
| `actions` | 操作 | 是 | 「打开」「移除」图标按钮；discovered 行的「移除」走 `assessDiscoveredRemoval`，`removable = false` 时按钮禁用 |

列显隐面板：表格右上「列」按钮弹出复选框列表；恒显列不可取消。

---

## 5. 按钮、标签、表单

| 元素 | 规格 |
| --- | --- |
| 主按钮 `.btn.primary` | 高 34px、圆角 6px、600 字重、背景 `--primary`、hover `--primary-hover`、active `--primary-pressed` |
| 次按钮 `.btn` | 边框 `--border`、背景 `--surface`、hover `--hover` |
| 危险按钮 `.btn.danger` | 背景 `--danger`，文字白 |
| 图标按钮 `.icon-btn` | 30px 方形、圆角 6px、hover `--primary-tint`；`.icon-btn.danger` hover `--danger-tint` |
| 标签 `.tag` | 高 22px、圆角 6px、12px 字、内边距 `0 8px`；`neutral` 背景 `#eef1f4` 文字 `--secondary`；`success` / `warning` / `danger` 用对应 tint 背景与主色文字 |
| 输入框 | 高 34px、边框 `--border`、圆角 6px、焦点 `outline: 2px solid rgba(43,108,176,.42)` |
| 焦点态（全局） | `outline: 2px solid rgba(43,108,176,.42); outline-offset: 1px` |
| 禁用态（全局） | `opacity: .5; cursor: not-allowed` |
| 胶囊 `.chip` | 高 26px、圆角 13px、背景 `--primary-tint`、右侧 `×` 可移除 |

图标一律内联 SVG 或文字符号：`▤`（备注）、`✎`（编辑）、`⟳`（刷新/同步）、`⠿`（拖拽手柄）、`▸`/`▾`（折叠箭头）、`×`（关闭）。

---

## 6. 模态层

- backdrop `rgba(22,27,32,.38)`，`position: fixed; inset: 0`。
- 面板 `width: min(720px, 90vw); max-height: 86vh; display: flex; flex-direction: column`，圆角 8px，背景 `--surface`。
- 页头：标题 17px；页体 `overflow: auto`；页脚背景 `#f8fafb`，按钮右对齐。
- `Esc` 关闭（危险确认类模态除外：移除、归档需点按钮）；打开时焦点移到面板。
- 三个模态：`RemovalDialog`、`MergeDetailDialog`、`ArchiveDialog`，统一用 `<Teleport to="body">`。

---

## 7. 页面与关键交互

### 7.1 工作区列表页

- 顶部：标题「工作区」、搜索框（按迭代号 / projectId / 分支名过滤，仅影响主列表）、「列」按钮。
- 迭代卡片头部（从左到右）：折叠箭头、迭代号（17px）、备注标记 `▤`（有备注才渲染，hover 显示气泡）、迭代路径（mono、省略）、右侧按钮组「✎ 备注 / 隐藏 / 归档 / 打开目录 / 检查合并」。
  - `manifestHealth ≠ valid`：卡片显示 danger 横条 + `manifestMessage`，「备注 / 隐藏 / 归档 / 检查合并」禁用。
- 卡片体：公共目录状态行（每条规则一个标签）+ 项目表格。
- 「复核状态」按钮：全局一个，位于页面标题右侧；点击时按钮进入 loading，旧列表保留，完成后整体替换；011 有回写时 toast「已同步 N 个分支重命名」。
- 「已隐藏迭代」收纳区：页面底部，标题「已隐藏迭代（N）」+ 展开箭头，默认折叠；无隐藏项时**整区不渲染**；不受搜索影响；展开后每项只显示迭代号 + 路径 + 「恢复」按钮，**不渲染项目表格**。
- 备注编辑：点击 `✎ 备注` 在卡片头内联展开单行输入框（maxlength 50）+「保存」「取消」；回车保存，Esc 取消。
- 无刷新按钮、无定时轮询。

### 7.2 创建页

- 上半区两列栅格 `.create-grid`（左：迭代号 + 最近迭代胶囊 + 备注；右：统一分支名 + 统一基分支 `<datalist>`）。
- 项目勾选表：列「选择 / 项目 / 类型 / vendor / 分支名 / 基分支」。勾选新项目时该行分支名与基分支**继承**当前统一值；修改统一值时**覆盖**所有已勾选行；单行修改不反向影响统一值。
- 基分支输入框旁 `⟳` 按钮：点击才发起 `ls-remote`；失败时按钮旁显示 warning 图标，`title` 放原始报错；候选降级为本地缓存。整页「⟳ 全部」按钮串行逐项目拉取。
- 底部固定操作条：「返回」「开始创建」；未选项目、迭代号非法、`workspaceRoot` 为空时禁用并给出原因。
- 点击开始创建后切换到 `OperationView`：每个项目一行，显示阶段标签与 `message`（含实际执行的命令）；全部结束后显示「返回列表」。

### 7.3 设置页

- 三块卡片：工作区根目录（必填标记、「选择目录」按钮、路径输入）；公共目录规则表（源目录、目标目录名、删除；「添加规则」）；项目表（标识、仓库路径、类型、vendor、删除；「添加项目」调用目录多选对话框 → `resolve_projects` → 逐条显示解析结果或错误）。
- 「保存」按钮：调用 `save_config`，成功后 toast「设置已保存」并用返回值回填表单；失败 toast 错误。
- 从列表移除项目只改配置，不动磁盘，UI 文案明示。

### 7.4 备注气泡（012）

`NoteTooltip.vue`：`<Teleport to="body">` + `position: fixed`；坐标由触发元素 `getBoundingClientRect()` 计算，默认显示在触发元素下方，间距 6px，超出视口底部时翻到上方；最大宽 320px，`white-space: pre-wrap; word-break: break-all`；`window` 上 `capture: true` 监听 `scroll` 与 `resize` 立即收起；组件卸载时移除监听。

### 7.5 拖拽（014）

- 手柄 `⠿` 是唯一 `draggable="true"` 元素，`<tr>` 不设 `draggable`。只有 `lifecycle = active` 的行渲染手柄；discovered、removed、createFailed 行不渲染手柄。
- `dragstart` 用 `dataTransfer.setData("text/plain", worktreePath)` 并设置 `effectAllowed = "move"`。
- 落点：目标行上半部分 → 插到该行之前；下半部分 → 插到下一行之前；拖到卡片表格空白区或表尾 → `beforeWorktreePath = null`（追加末尾）。落点行显示 2px `--primary` 上边线指示。
- 落点是自己原位（`before` 等于自身或自身的下一行）时**不发请求**。
- 同迭代 → `reorderProject`；跨迭代 → `moveProject`。`manifestHealth ≠ valid` 的迭代不接受落点（`dragover` 不 `preventDefault`）。
- 成功后清掉源与目标迭代的合并检查结论，重新 `listWorkspaces(false)`。

---

## 8. 文案规范

- 全部中文；Git 术语、标识符、路径、命令保持英文原文。
- 错误信息直接显示后端 `message`，前端不改写、不翻译。
- 标签文案对照（`utils/status.ts`、`utils/merge.ts`）：

| 枚举 | 文案 | 色 |
| --- | --- | --- |
| `valid` | 有效 | success |
| `unknown` | 未复核 | neutral |
| `missingDirectory` | 目录缺失 | danger |
| `notRegistered` | 未注册 | danger |
| `headMismatch` | HEAD 不一致 | warning |
| `sourceMissing` | 源仓库缺失 | danger |
| `removed` | 已移除 | neutral |
| `discovered` | 未托管 | warning |
| `merged` | 已合并 | success |
| `contained` | 已包含（疑似 squash） | success |
| `unmerged` | 未合并 | warning |
| `targetMissing` | 目标分支不存在 | neutral |
| `branchMissing` | 分支不存在 | danger |
| `notCheckable` | 无法检查 | neutral |
| `error` | 检查出错 | danger |
| `copied` | 已复制 | success |
| `notPhp` | 无需复制 | neutral |
| `sourceMissing`(vendor) | 源无 vendor | neutral |
| `lockMissing` | 缺少 lock | warning |
| `lockMismatch` | lock 不一致 | warning |
| `targetExists` | 目标已存在 | neutral |
| `copyFailed` | 复制失败 | danger |
| `pending` / `reused` / `copied` / `failed`（公共目录） | 待复制 / 已复用 / 已复制 / 失败 | neutral / neutral / success / danger |

---

## 9. localStorage 键

| 键 | 值 | 默认 |
| --- | --- | --- |
| `worktree-manager.visible-columns` | JSON 数组，列 key | 全部可见 |
| `worktree-manager.expanded-iterations` | JSON 数组，展开的迭代号 | 全部展开（键不存在时） |
| `worktree-manager.hidden-zone-expanded` | `"1"` / `"0"` | `"0"` |

读取失败（JSON 损坏）时回退默认值并覆盖写入。

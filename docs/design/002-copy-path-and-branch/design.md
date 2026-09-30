# 002 · 复制路径与分支

> 设计文档是**当次决策的历史记录**，设计确认后不回改正文；实现偏离时在 §7「实现记录」说明。
> 相关基线：`docs/ui-spec.md §4 §4.1 §7.5`、`docs/architecture.md §3 §4.1`、`docs/data-model.md §5`。

---

## 1. 背景与目标

### 现状

列表里 `worktreePath` 与 `branchDisplay` 是单行省略显示（`max-width: 230px`），用户要拿完整路径去终端 `cd`、拿分支名去 PR 页面时只能靠 `title` 提示或双击选择再复制，路径含 `/` 时双击选不全。

### 目标

- 单击「Worktree 路径」「分支 / HEAD」两格，整格内容写入系统剪贴板。
- 复制成功后该格短暂显示「已复制」。
- 纯前端实现，不新增 Tauri 命令、不占操作锁、不改 capability。

### 非目标

- 不复制其他列（`baseCommit`、`source` 等）。
- 不提供「复制为 `cd` 命令」等格式化变体。
- 不做右键菜单。

---

## 2. 已确认决策

1. **纯前端**：用 `navigator.clipboard.writeText`，失败时降级到隐藏 `<textarea>` + `document.execCommand("copy")`。
   - 被否决：新增 `copy_to_clipboard` 后端命令 / 引入 `tauri-plugin-clipboard-manager`。原因：依赖白名单不含该插件；WebView 内 `navigator.clipboard` 在 Tauri 安全上下文下可用，无需扩大能力面。
2. **单击整格即复制，总是复制**（不判断当前 selection 是否为空）。
   - 被否决：`click` 时若 `window.getSelection()` 非空则不复制（让用户手工选择部分文本）。原因：选择后松开鼠标同样触发 `click`，判断 selection 会让「单击复制」在用户不小心轻微拖动时失效，行为不可预测；整格复制是本功能的核心承诺。用户仍可用键盘 `Shift+方向键` 或三击选择。
3. **`mousedown` 不 `preventDefault`**，保证文本仍可被鼠标选择与拖选；只在 `click` 上执行复制。
4. **「已复制」反馈**：1.2 秒内该格内容替换为「已复制」标签（不叠加、不 toast），到时恢复；快速连点重置计时器。
   - 被否决：toast。原因：连续复制多格时 toast 堆叠干扰；就地反馈更清晰。
5. **复制内容**：`worktreePath` 格复制 `WorkspaceProject.worktreePath` 原文；`branch` 格复制 `branchDisplay`（`detached @ <短hash>` 时复制完整 40 位 `baseCommit`？——**否**，复制 `branchDisplay` 原文，与所见一致）。
   - 被否决：detached 时复制完整 hash。原因：所见即所得原则；完整 hash 已在 `baseCommit` 列 `title` 中。
6. **与 014 互不干扰**：拖拽只从手柄 `⠿` 起（`<tr>` 不 `draggable`），复制格不在手柄上；拖拽过程中（`dragging` 状态）忽略 `click`。
7. 复制失败（两条路径都失败）→ toast danger「复制失败，请手动选择文本」，不抛错到横幅。

---

## 3. 技术方案概要

### 3.1 数据结构

只读使用 `WorkspaceProject.worktreePath`、`WorkspaceProject.branchDisplay`（`data-model.md §5`）。无新增字段、无持久化。

### 3.2 后端

无。不新增命令、不占锁、不改 `capabilities/default.json`。

### 3.3 前端

`src/utils/clipboard.ts`：

```ts
/** 写入剪贴板；成功 resolve，两种方式都失败 reject */
export async function copyText(text: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(text); return }
  } catch { /* 走降级 */ }
  const ta = document.createElement("textarea")
  ta.value = text
  ta.setAttribute("readonly", "")
  ta.style.position = "fixed"; ta.style.left = "-9999px"; ta.style.top = "0"
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try { ok = document.execCommand("copy") } finally { document.body.removeChild(ta) }
  if (!ok) throw new Error("copy failed")
}
```

`IterationCard.vue`（项目表格所在组件）：

- 两个可复制格加 `class="copyable"`、`@click="onCopy(row.worktreePath, cellKey)"`、`title` 全文。
- 局部状态 `copiedKey = ref<string | null>(null)`，键为 `${worktreePath}:${column}`；`onCopy` 成功后设值并 `setTimeout(1200)` 清空，重复点击先 `clearTimeout`。
- 模板：`copiedKey === key ? <span class="tag success">已复制</span> : 原内容`。
- `dragging`（014 的局部状态）为 true 时 `onCopy` 直接返回。
- 组件卸载时清 timer。

样式（`styles.css`）：`.copyable { cursor: copy; } .copyable:hover { background: var(--primary-tint); }`。

SSR 测试（`scripts/test-frontend.mjs`）：`copyText` 在无 `navigator` 环境下应 reject 而不是抛同步异常（用 `globalThis.navigator = undefined` 与 `document` 缺失模拟）；`IterationCard` 渲染出的路径格含 `class="copyable"`。

### 3.4 事件

无。

---

## 4. 状态与枚举

无后端枚举。前端局部状态：

| 状态 | 取值 | 含义 |
| --- | --- | --- |
| `copiedKey` | `string \| null` | 正在显示「已复制」的格 |
| `dragging` | `boolean`（014 所有） | 为真时忽略 click |

---

## 5. 测试要点（对应 `implementation-plan.md` S5 第 4 项）

1. `copyText` 优先走 `navigator.clipboard.writeText`（mock 后断言被调用且未创建 textarea）。
2. `writeText` reject 时降级到 `execCommand`（mock `document.execCommand` 返回 true，断言 textarea 已创建又已移除）。
3. 两条路径都失败 → reject。
4. 渲染断言：`worktreePath`、`branch` 两格含 `copyable`，其他列不含。
5. 手工验证（真实窗口）：单击路径格 → 终端粘贴得到完整路径；单击分支格 → 得到分支名；「已复制」1.2 秒后恢复；拖选文本仍可用；拖动手柄时不触发复制。

---

## 6. 已知局限

- `execCommand("copy")` 已被浏览器标为废弃，未来 WebView 版本可能移除，届时只剩 `navigator.clipboard` 路径。
- 复制的是显示文本：detached 记录复制到的是 `detached @ <短hash>`，不是完整 hash。
- 不支持一次复制多行/多格。
- 「已复制」替换显示期间该格宽度可能抖动（短文本），接受。
- 无剪贴板权限（极少数受管环境）时只能提示失败，无进一步降级。

---

## 7. 实现记录

- 完成日期：2026-09-30（S5）
- 偏离项：无。`copyText` 按设计返回 `Promise<void>`（两种方式都失败时 reject）；detached 行复制 `detached @ <短hash>` 原文，与所见一致。
- 原因：—
- 涉及文件：`src/utils/clipboard.ts`、`src/components/IterationCard.vue`、`src/styles.css`、`scripts/test-frontend.mjs`

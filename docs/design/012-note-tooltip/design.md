# 012 · 备注气泡（NoteTooltip）

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 依赖：009 迭代备注（清单 `note` 字段与卡片头部 `▤` 标记已存在）。

---

## 1. 背景与目标

### 现状

009 落地后，迭代卡片头部在 `note` 非空时渲染一个 `▤` 标记。备注最长 50 字符，卡片头部只放得下标记本身，用户看不到全文。迭代卡片的根元素为了裁掉圆角内的表格与横向滚动条设置了 `overflow: hidden`，卡片内任何绝对定位的浮层都会被裁切；折叠状态的卡片高度只有一行头部，裁切更严重。

### 目标

- 鼠标悬浮或键盘聚焦到 `▤` 标记时，在标记附近显示备注全文。
- 气泡在任何卡片状态（展开 / 折叠）、任何页面滚动位置下都不被裁切。
- 页面滚动或窗口尺寸变化时气泡立即收起，避免气泡与触发元素错位。
- 纯前端实现，不新增后端命令，不占操作锁。

### 非目标

- 不在气泡内编辑备注（编辑走 009 的 `✎ 备注` 内联输入框）。
- 不做通用 tooltip 组件库，只服务备注标记这一个场景。
- 不支持富文本、链接、多段落。

---

## 2. 已确认决策

1. **气泡用 `<Teleport to="body">` 渲染，`position: fixed`。**
   被否决备选：在卡片内用 `position: absolute` 定位气泡。原因：卡片根元素 `overflow: hidden` 会裁掉气泡；折叠卡片下气泡几乎完全不可见；给卡片改成 `overflow: visible` 又会破坏表格横向滚动与圆角裁切。
2. **坐标由触发元素的 `getBoundingClientRect()` 计算，默认显示在触发元素下方，间距 6px；气泡底部超出视口时翻到上方。**
   被否决备选：固定显示在上方。原因：列表页第一张卡片贴近内容区顶部，上方空间不足会被顶部横幅或标题遮挡。
3. **`window` 上以 `capture: true` 监听 `scroll` 与 `resize`，触发即收起。**
   被否决备选：滚动时重新计算坐标跟随。原因：内容区 `overflow: auto` 与多个 `.table-scroll` 都可能滚动，跟随需要监听所有滚动容器并逐帧重算；收起是零成本且行为可预期。`capture: true` 是为了捕获内容区等非 `window` 元素的滚动事件（`scroll` 不冒泡）。
4. **hover 与 focus 都触发显示；`mouseleave` 与 `blur` 收起。触发元素 `tabindex="0"`，`aria-describedby` 指向气泡 id。**
   被否决备选：只响应 hover。原因：键盘用户无法查看备注；`ui-spec.md §5` 要求统一焦点态，focus 触发与之一致。
5. **气泡样式：`max-width: 320px`、`white-space: pre-wrap`、`word-break: break-all`、背景 `--surface`、边框 `--border`、圆角 6px、阴影 `0 4px 12px rgba(22,27,32,.12)`、`z-index: 1000`（高于模态 backdrop 之下的所有内容，低于模态面板 `1100`）。**
   被否决备选：深色气泡。原因：`ui-spec.md` 未定义深色 token，不引入新颜色。
6. **不做显示延时（无 hover delay）。** 见 §6。
7. **组件卸载（`onBeforeUnmount`）时移除全部 `window` 监听。**

---

## 3. 技术方案概要

### 数据结构

使用 `data-model.md §5` 的 `WorkspaceGroup.note: string | null`。本功能不新增任何持久化字段、不新增类型。

### 后端

无。不新增 Tauri 命令、不触碰操作锁。

### 前端

**入口组件**：`src/components/NoteTooltip.vue`，由 `IterationCard.vue` 在 `group.note !== null` 时渲染于卡片头部 `▤` 标记处。

Props / 结构：

```ts
// NoteTooltip.vue  <script setup lang="ts">
defineProps<{ note: string; id: string }>()   // id 用于 aria-describedby，建议 `note-tip-${iteration}`
```

模板骨架：

```vue
<span
  class="note-mark"
  tabindex="0"
  :aria-describedby="open ? id : undefined"
  @mouseenter="show" @mouseleave="hide" @focus="show" @blur="hide"
>▤</span>
<Teleport to="body">
  <div v-if="open" :id="id" role="tooltip" class="note-tooltip" :style="style">{{ note }}</div>
</Teleport>
```

坐标算法（`show()`）：

```ts
const rect = trigger.value.getBoundingClientRect()
const GAP = 6
let top = rect.bottom + GAP
// 先按下方渲染，nextTick 后读取气泡高度；若 top + height > window.innerHeight 则翻到上方
//   top = rect.top - GAP - height
let left = rect.left
// 若 left + 320 > window.innerWidth 则 left = window.innerWidth - 320 - 8
style.value = { top: `${top}px`, left: `${left}px` }
```

监听：

```ts
onMounted(() => {
  window.addEventListener('scroll', hide, { capture: true, passive: true })
  window.addEventListener('resize', hide, { passive: true })
})
onBeforeUnmount(() => {
  window.removeEventListener('scroll', hide, { capture: true })
  window.removeEventListener('resize', hide)
})
```

`hide` 是同一个函数引用，保证 `removeEventListener` 生效。

**状态存放**：组件内 `ref<boolean>(open)` 与 `ref<CSSProperties>(style)`，不进 `App.vue`，不进 localStorage。

**样式**：`.note-mark` 与 `.note-tooltip` 写在 `src/styles.css`（Teleport 到 body 的元素不受组件 scoped 样式影响，本项目统一不使用 scoped 样式）。

**api/tauri.ts**：不涉及。

### 事件

无。

---

## 4. 状态与枚举

本功能不涉及任何 `data-model.md §3` 枚举。组件内部状态：

| 状态 | 取值 | 含义 |
| --- | --- | --- |
| `open` | `true / false` | 气泡是否渲染 |
| `placement` | `below / above` | 当前放置方向，仅用于调试与测试断言（class `note-tooltip--above`） |

---

## 5. 测试要点

对应 `implementation-plan.md` S7。

前端 SSR 测试（`scripts/test-frontend.mjs`）：

- `NoteTooltip` 以 `note="hello"` 渲染，关闭态 HTML 含 `▤` 且**不含** `hello`（气泡未渲染）。
- 触发元素 HTML 含 `tabindex="0"`。
- `IterationCard` 在 `note: null` 时 HTML 不含 `note-mark`；`note: "x"` 时含。

真实窗口手工验证（不可省略，SSR 覆盖不到定位与事件）：

- 展开卡片 hover `▤` → 气泡出现在标记下方约 6px，内容为全文。
- 折叠卡片 hover → 气泡完整可见，不被卡片裁切。
- 页面滚到底部最后一张卡片 hover → 若下方空间不足，气泡出现在上方。
- 气泡打开时滚动内容区 → 立即收起；拖动窗口尺寸 → 立即收起。
- Tab 键聚焦到 `▤` → 气泡出现；再 Tab 离开 → 收起。
- 50 字符无空格备注 → 气泡宽度不超过 320px，文本 `break-all` 换行。
- 切到设置页再切回列表页 → 无残留气泡、无重复监听（DevTools `getEventListeners(window).scroll` 数量等于当前渲染的标记数）。

---

## 6. 已知局限

- 气泡是纯展示元素（`mouseleave` 即收起），用户无法把鼠标移入气泡选中并复制文字；需要复制备注时应使用 009 的编辑框。
- 没有显示 / 隐藏延时：鼠标快速掠过多个标记时气泡会连续闪现；触发元素与气泡之间留 6px 间隙，鼠标从标记移向气泡的瞬间会因 `mouseleave` 收起。
- 只在 `scroll` / `resize` 时收起，不跟随；任何其他导致触发元素位移的布局变化（如同一卡片内备注编辑框展开）不会自动重算，需要用户再次 hover。
- 气泡 `z-index` 低于模态面板；模态打开期间气泡不会出现在模态之上（此时也不应该能 hover 到标记）。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

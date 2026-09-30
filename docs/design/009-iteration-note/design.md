# 009 · 迭代备注

> 设计文档是**当次决策的历史记录**。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 基线：`docs/requirements.md §2.2 / §3`、`docs/data-model.md §2`、`docs/architecture.md §6-7`、`docs/ui-spec.md §7.1`。

---

## 1. 背景与目标

**现状**：迭代卡片只有迭代号与路径，用户无法给一个迭代记下「这批是给客户 X 的热修」这类一句话说明。迭代号本身（如 `7.3.0`）不足以在 10+ 个迭代里快速辨认。

**目标**：

- 每个迭代可以写一条 ≤50 字符的单行备注，持久化在迭代清单 `note` 字段。
- 列表卡片头部用 `▤` 标记「有备注」，hover 显示全文（气泡由 012 实现）；无备注时**没有任何常驻痕迹**。
- 备注可在卡片头内联编辑；空串保存＝清除。
- 创建工作区时可以顺带写备注，但**缺省不改动**已有备注。

**非目标**：

- 不支持多行、富文本、Markdown。
- 不记录备注修改历史、修改人、修改时间。
- 不给单个项目（worktree 记录）加备注。
- 不做备注搜索以外的任何索引（搜索框按迭代号 / projectId / 分支名过滤，备注不参与过滤）。

---

## 2. 已确认决策

1. **备注存在迭代清单里，不存全局配置。**
   被否决备选：存 `config.json` 的 `iterationNotes: { [iteration]: note }`。原因：清单是迭代的唯一事实来源；迭代目录被移走/删除后全局配置会残留脏数据；两份文件写入无法原子。

2. **长度上限 50，按 Unicode 标量（Rust `chars().count()`）计数，不按字节、不按 grapheme。**
   被否决备选：按字节（UTF-8 中文 3 字节，50 字节只够 16 个汉字）；按 grapheme cluster（需要额外 crate，违反依赖白名单）。原因：标量计数在 Rust 与 JS（`[...str].length`）两侧都能零依赖一致实现。

3. **单行：含 `\n` 或 `\r` 直接拒绝（validation 错误），不静默替换成空格。**
   被否决备选：把换行替换为空格后保存。原因：静默改写用户输入违反「后端不擅自改写数据」原则，用户会困惑为什么保存的和输入的不一样。

4. **保存前 `trim` 首尾空白；trim 后为空串 → 视为清除（写 `null`）；入参 `null` → 清除。**
   被否决备选：空串与 `null` 区分（空串＝保存空备注）。原因：空备注与无备注对用户没有区别，区分只会让 `▤` 标记出现在一条看不见的备注上。

5. **`CreateRequest.note` 字段缺省（`undefined` / 序列化时不出现）＝不改动已有备注；显式 `null` ＝清除；显式非空字符串 ＝ 覆盖。**
   被否决备选：用 `note: ""` 表达「不改动」。原因：`""` 在 `set_iteration_note` 里已经是「清除」的语义，同一个值在两个命令里含义相反必然出 bug；且前端表单空输入框自然产出 `""`，会把「用户没填」误当「用户要清除」。实现上 Rust 侧用 `#[serde(default, skip_serializing_if = "Option::is_none")] note: Option<Option<String>>` 或等价的三态包装区分「缺省 / null / 有值」。

6. **`set_iteration_note` 持全局操作锁。**
   被否决备选：不持锁（备注写入很快）。原因：它要原子改写清单，与创建 / 移除 / 复核回写（011）同时写同一份清单会互相覆盖字段；锁是非阻塞的，用户感知只是「已有操作执行中」的 toast。

7. **清单 `manifestHealth ≠ valid` 时拒绝写备注，不重建清单。**
   被否决备选：清单缺失时新建一份只含 `note` 的清单。原因：`architecture.md §6.1` 规定损坏清单只报错不重建；缺失清单意味着该目录不是本工具创建的迭代，不应凭空生成托管记录。

8. **前端 `utils/note.ts` 的校验只用于即时提示与禁用保存按钮，后端仍然完整校验。**
   被否决备选：前端校验通过就信任。原因：前端不可信原则（`architecture.md §4.3`）。

9. **UI 用内联编辑而不是模态框。**
   被否决备选：弹出「编辑备注」模态。原因：单行 50 字符的输入不值得一个模态；内联编辑保持视线不离开卡片。

---

## 3. 技术方案概要

### 3.1 数据结构

- `Manifest.note: Option<String>`（`data-model.md §2`，已存在字段）。
- `WorkspaceGroup.note: Option<String>`（`data-model.md §5`，投影字段，快扫与复核都带出）。
- `CreateRequest.note`：三态，见决策 5。
- 无新增结构。

### 3.2 后端

模块：`manifest.rs`（清单读写）、`lib.rs`（命令适配 + 锁）、`workspace.rs`（创建时写入）。

```rust
// validation.rs
/// 备注规范化：trim → 空则 None → 含 \n/\r 拒绝 → chars().count() > 50 拒绝
pub fn normalize_note(input: Option<&str>) -> Result<Option<String>, AppError>;

// manifest.rs
/// 读取清单，health 非 valid 时返回 ManifestDamaged / NotFound
pub fn load_manifest_strict(iteration_dir: &Path) -> Result<Manifest, AppError>;
/// 只改 note 字段后原子写回
pub fn update_note(iteration_dir: &Path, note: Option<String>) -> Result<Option<String>, AppError>;

// lib.rs（命令适配层）
#[tauri::command]
async fn set_iteration_note(state: State<'_, OperationState>, iteration: String, note: Option<String>)
    -> Result<Option<String>, AppErrorObject>;
```

执行序列（`set_iteration_note`）：

1. `state.acquire("设置备注")`，占用 → `busy`。
2. `validation::validate_iteration(&iteration)`。
3. 读配置，`workspaceRoot` 为 `null` → `validation` 错误「请先在设置中填写工作区根目录」。
4. `path_utils::join_segments(root, [iteration])` → `ensure_within(root, dir)`。
5. `manifest::load_manifest_strict(dir)`；`missing` → `notFound`，`damaged` → `manifestDamaged`。
6. `validation::normalize_note(note.as_deref())`。
7. `manifest.note = normalized; atomic_json::write_json_atomic(...)`。
8. 返回 `manifest.note`。

创建流程（`workspace.rs::create_batch`）：迭代目录已存在且清单可读时，仅当 `request.note` 不是「缺省」态才调用 `normalize_note` 并覆盖；新建清单时 `note` 取 `request.note` 规范化结果（缺省 → `null`）。校验失败在公共目录复制之前就返回，不产生半成品。

Git 调用：**无**。

### 3.3 前端

- 入口组件：`IterationCard.vue` 头部 `✎ 备注` 按钮 → 内联输入框（`maxlength="50"`，`Enter` 保存、`Esc` 取消、失焦不自动保存）+「保存」「取消」按钮；`manifestHealth ≠ valid` 时按钮禁用。
- 有备注时头部渲染 `▤`（`title` 为备注全文；hover 气泡见 012）；无备注时不渲染任何元素。
- `api/tauri.ts`：`setIterationNote(iteration: string, note: string | null): Promise<string | null>`。
- `utils/note.ts`：`validateNote(input: string): { ok: boolean; message?: string; normalized: string | null }`，实现与后端相同的 trim / 标量计数 / 换行检查，只用于实时提示。
- 状态：编辑态 `editing: boolean` 与草稿 `draft: string` 存 `IterationCard.vue` 局部 `ref`；保存成功后用返回值更新父组件传入的 `group.note`（通过 emit `note-updated`，由 `WorkspaceList.vue` 就地替换该 group 的 `note`，**不重新拉列表**）。
- `CreateWorkspace.vue`：备注输入框；提交时输入框为空且用户未触碰 → 请求中**不带** `note` 字段；用户清空过 → 带 `note: null`；有内容 → 带字符串。
- 演示模式：`setIterationNote` 返回 `Promise.reject({ code: "unsupported", ... })`。

### 3.4 事件

无。

---

## 4. 状态与枚举

本功能不新增枚举。涉及：

| 枚举 / 字段 | 取值 | 含义 |
| --- | --- | --- |
| `ManifestHealth` | `valid` | 允许写备注 |
| `ManifestHealth` | `missing` / `damaged` | 拒绝写备注，分别映射 `notFound` / `manifestDamaged` |
| `ErrorCode` | `validation` | 超长、含换行、迭代号非法、根目录未设置 |
| `ErrorCode` | `busy` | 操作锁被占用 |
| `note` | `string \| null` | `null`＝无备注 |

---

## 5. 测试要点

Rust（`validation.rs` / `manifest.rs` 单测，S6）：

- `normalize_note(Some("  hello  "))` → `Some("hello")`。
- `normalize_note(Some("   "))` → `None`；`normalize_note(Some(""))` → `None`；`normalize_note(None)` → `None`。
- 50 个汉字通过；51 个汉字拒绝（`validation`）；50 个 ASCII 通过。
- 含 `\n`、`\r`、`\r\n` 均拒绝。
- `update_note` 后清单其他字段（`projects`、`sharedDirectories`、`hiddenAt`、`createdAt`）字节级不变、`note` 已更新；临时文件不残留。
- 清单缺失 → `notFound`；清单 JSON 损坏 → `manifestDamaged` 且文件内容不变。
- 操作锁占用时 `set_iteration_note` 返回 `busy`。
- 创建：`note` 缺省 → 已有备注保持；`note: null` → 清除；`note: "x"` → 覆盖；新建清单 + 缺省 → `null`。

前端（`scripts/test-frontend.mjs`）：

- `validateNote` 与后端规则一致的 6 组输入输出。
- `IterationCard` 在 `note: null` 时 SSR 输出不含 `▤`；`note: "abc"` 时含 `▤`。

---

## 6. 已知局限

- 不支持富文本与换行；需要多行说明的场景只能写到外部工具。
- 无修改历史、无修改时间、无修改人；覆盖即丢失。
- 备注不参与搜索过滤。
- 50 字符按 Unicode 标量计数，组合字符（如带变音符号的字母、部分 emoji 序列）会被计为多个。
- 清单缺失的迭代目录无法写备注（需要先由工具创建过）。

---

## 7. 实现记录

- 完成日期：2026-09-30（S6）
- 偏离项：`normalize_note` 实现在 `workspace.rs`（与创建流程共用），并按 `workflows.md §10` 使用文案「备注不能换行」「备注不能超过 50 字符」；前端 `utils/note.ts` 文案与后端一致。
- 原因：创建流程（workspace.rs）已经需要同一条校验，放在同一模块避免两份实现；文案取当前态文档。
- 涉及文件：`src-tauri/src/workspace.rs`、`src-tauri/src/manifest.rs`、`src-tauri/src/lib.rs`、`src/utils/note.ts`、`src/App.vue`、`src/components/IterationCard.vue`

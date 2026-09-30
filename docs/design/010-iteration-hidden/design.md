# 010 · 迭代隐藏

> 设计文档是**当次决策的历史记录**。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 基线：`docs/requirements.md §3.10`、`docs/data-model.md §2 / §5`、`docs/architecture.md §6-7`、`docs/ui-spec.md §7.1 / §9`。

---

## 1. 背景与目标

**现状**：主列表按迭代目录平铺，做完但尚未归档（008）的迭代、暂停的迭代都和进行中的迭代混在一起，越用越长。归档（008）会移除 worktree，是重操作；用户需要一个「只是先收起来」的轻操作。

**目标**：

- 迭代可以被「隐藏」：清单写 `hiddenAt` 时间戳，其他一切不变。
- 隐藏的迭代从主列表移到页面底部「已隐藏迭代」收纳区，可一键「恢复」。
- 在已隐藏的迭代上继续创建 worktree 时自动取消隐藏（用户显然又在用它了）。
- 已归档（`archivedAt` 非空）优先级高于隐藏：归档的迭代两个区域都不出现。

**非目标**：

- 不影响任何后端行为：隐藏的迭代仍可复核、检查合并、移除、归档、作为拖拽落点（只要清单有效）。
- 不做审计（谁隐藏、为什么隐藏）。
- 不做「按条件自动隐藏」。
- 不做全局配置层面的隐藏列表。

---

## 2. 已确认决策

1. **隐藏是清单字段 `hiddenAt`，不是前端 localStorage。**
   被否决备选：只在 localStorage 存隐藏的迭代号集合。原因：迭代属性应随迭代目录走（换机器、重装应用都应保持）；localStorage 只放纯视图偏好（列显隐、折叠态）。

2. **`hiddenAt` 只是时间戳，恢复即写 `null`，不保留「上次隐藏时间」。**
   被否决备选：`hidden: boolean` + `hiddenAt` 两个字段。原因：一个可空时间戳同时表达「是否隐藏」与「何时隐藏」，少一个可能不一致的字段。

3. **后端投影仍然返回隐藏的迭代（`WorkspaceGroup.hiddenAt` 非空），由前端分区。**
   被否决备选：`list_workspaces` 增加 `includeHidden` 参数、隐藏的迭代不返回。原因：收纳区需要展示隐藏项并提供恢复按钮，前端反正要拿到；一次调用拿全量最简单，也不需要两套快扫逻辑。归档的迭代则完全不返回（`requirements.md §3.10`），两者刻意区分。

4. **创建时自动取消隐藏。**
   被否决备选：创建时保持隐藏并 toast 提示「该迭代已隐藏」。原因：用户在已隐藏的迭代上创建，说明它已回到进行中；创建成功后又找不到它会造成困惑。实现位置在 `workspace.rs::create_batch` 初始化清单阶段（写 `hiddenAt = null`），不论后续项目是否失败。

5. **`set_iteration_hidden` 持全局操作锁。**
   被否决备选：不持锁。原因：同 009，避免与其他清单写入互相覆盖。

6. **清单 `manifestHealth ≠ valid` 时拒绝隐藏。**
   被否决备选：清单缺失/损坏的迭代允许通过其他方式隐藏。原因：无处写 `hiddenAt`，且损坏清单只报错不重建（`architecture.md §6.1`）；这类迭代本来就以异常态显示，用户应先修复。

7. **收纳区默认折叠、展开态记 localStorage（`worktree-manager.hidden-zone-expanded`），不受搜索影响，不渲染项目明细表。**
   被否决备选：收纳区完整渲染卡片（含项目表格）。原因：隐藏的目的就是减少视觉噪音；展开后只需要「它在这里 + 恢复」两个信息。搜索不影响收纳区是为了避免「搜不到的隐藏项是被过滤了还是被恢复了」的歧义。

8. **无隐藏项时整个收纳区（含标题）不渲染。**
   被否决备选：始终渲染标题「已隐藏迭代（0）」。原因：常驻空区块是噪音，与「无备注无痕迹」保持同一原则。

---

## 3. 技术方案概要

### 3.1 数据结构

- `Manifest.hiddenAt: Option<DateTime<Utc>>`（序列化 RFC 3339 字符串）。
- `WorkspaceGroup.hiddenAt: Option<String>`（投影透传）。
- 无新增结构。

### 3.2 后端

模块：`manifest.rs`、`lib.rs`、`workspace.rs`。

```rust
// manifest.rs
/// hidden=true 写 Utc::now()，false 写 None；只改 hiddenAt 字段后原子写回；返回新值
pub fn update_hidden(iteration_dir: &Path, hidden: bool) -> Result<Option<DateTime<Utc>>, AppError>;

// lib.rs
#[tauri::command]
async fn set_iteration_hidden(state: State<'_, OperationState>, iteration: String, hidden: bool)
    -> Result<Option<String>, AppErrorObject>;
```

执行序列（`set_iteration_hidden`）：

1. `state.acquire("隐藏迭代")` / `"恢复迭代"`，占用 → `busy`。
2. `validate_iteration`；读配置；根目录未设置 → `validation`。
3. `join_segments` + `ensure_within`。
4. `load_manifest_strict`：`missing` → `notFound`，`damaged` → `manifestDamaged`。
5. `archivedAt` 非空 → `conflict`「已归档的迭代不能隐藏或恢复」（正常 UI 不会触发，防御前端伪造）。
6. 写 `hiddenAt`，原子写回，返回字符串或 `null`。

`list_groups`（快扫与复核）：`archivedAt` 非空 → 跳过；否则正常投影并带出 `hiddenAt`。

`workspace.rs::create_batch`：加载/初始化清单后，若 `hiddenAt` 非空则置 `null`，随第一次清单写入落盘（在公共目录处理之前的初始化写入）。

Git 调用：**无**。

### 3.3 前端

- `WorkspaceList.vue`：
  - `visibleGroups = groups.filter(g => !g.hiddenAt).filter(searchMatch)`。
  - `hiddenGroups = groups.filter(g => !!g.hiddenAt)`（**不**经过 `searchMatch`）。
  - `hiddenGroups.length === 0` → 不渲染收纳区任何 DOM。
  - 收纳区：标题「已隐藏迭代（N）」+ 折叠箭头；展开态 `hiddenZoneExpanded` 由 `utils/storage.ts` 读写键 `worktree-manager.hidden-zone-expanded`（`"1"`/`"0"`，默认 `"0"`）。
  - 展开后每项一行：迭代号、路径（mono、省略）、`hiddenAt` 本地时间、「恢复」按钮。**不渲染** `IterationCard` 的项目表格与按钮组。
- `IterationCard.vue` 头部「隐藏」按钮 → `setIterationHidden(iteration, true)`；`manifestHealth ≠ valid` 时禁用。
- 收纳区「恢复」按钮 → `setIterationHidden(iteration, false)`。
- 两者成功后：用返回值就地更新该 group 的 `hiddenAt`（emit 到 `WorkspaceList`），**不重新拉列表**；toast「已隐藏 7.3.0」/「已恢复 7.3.0」。失败 toast 后端 `message`。
- `api/tauri.ts`：`setIterationHidden(iteration: string, hidden: boolean): Promise<string | null>`。
- 搜索框只作用于主列表；搜索有结果为空时主列表显示「无匹配迭代」，收纳区照常。
- 演示模式：写命令 `reject({ code: "unsupported" })`。

### 3.4 事件

无。

---

## 4. 状态与枚举

本功能不新增枚举。涉及：

| 字段 / 枚举 | 取值 | 含义 |
| --- | --- | --- |
| `hiddenAt` | `null` | 显示在主列表 |
| `hiddenAt` | RFC 3339 字符串 | 显示在收纳区 |
| `archivedAt` | 非空 | 不投影，任何区域都不出现（优先于 `hiddenAt`） |
| `ManifestHealth` | `missing` / `damaged` | 拒绝隐藏/恢复 |
| `ErrorCode` | `busy` / `notFound` / `manifestDamaged` / `conflict` / `validation` | 见 §3.2 |

---

## 5. 测试要点

Rust（S6）：

- `update_hidden(dir, true)` → 清单 `hiddenAt` 为合法 RFC 3339 且与当前时间相差 < 5 秒；其他字段不变。
- `update_hidden(dir, false)` → `hiddenAt = null`。
- 清单缺失 → `notFound`；损坏 → `manifestDamaged` 且文件不变。
- `archivedAt` 非空 → `conflict`。
- `list_groups`：`hiddenAt` 非空的迭代**在**输出中且字段透传；`archivedAt` 非空的迭代**不在**输出中；两者都非空 → 不在。
- `create_batch` 在 `hiddenAt` 非空的迭代上创建（即使项目全部失败）→ 清单 `hiddenAt = null`。
- 操作锁占用 → `busy`。

前端（`scripts/test-frontend.mjs`）：

- `WorkspaceList` 传入 0 个隐藏 group → SSR 输出不含「已隐藏迭代」。
- 传入 2 个隐藏 group → 含「已隐藏迭代（2）」，且折叠态下不含这两个迭代的项目表格。
- `utils/storage.ts` 读取非法值回退 `"0"`。

真实窗口：隐藏 → 主列表消失、底部出现收纳区；展开 → 恢复 → 回到主列表；重启后展开态保持。

---

## 6. 已知局限

- 只有 `hiddenAt` 时间戳，无「谁、为何」等审计信息。
- 隐藏不改变任何后端行为：隐藏的迭代仍会参与复核（跑 Git），大量隐藏迭代会拖慢复核；这是刻意的（隐藏≠归档）。
- 收纳区不响应搜索；想找某个隐藏迭代只能展开后肉眼看。
- 清单缺失或损坏的迭代无法隐藏，只能修复清单或移走目录。
- 恢复时不校验目录是否仍存在；目录已丢的迭代恢复后会以异常态显示在主列表。

---

## 7. 实现记录

- 完成日期：2026-09-30（S6）
- 偏离项：锁名统一为「隐藏迭代」（设计 §3.2 写的是「隐藏迭代」/「恢复迭代」两个名字，取 `architecture.md §6.2` 的单一名字）；
  前端隐藏 / 恢复成功后按 `workflows.md §9` 第 5、7 步就地更新 `hiddenAt`（不重新快扫）。
- 原因：锁名以当前态文档为准；就地更新避免多余的全工作区扫描。
- 涉及文件：`src-tauri/src/manifest.rs`、`src-tauri/src/lib.rs`、`src/App.vue`、`src/components/WorkspaceList.vue`

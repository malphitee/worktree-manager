# 014 · 项目拖拽排序与跨迭代移动

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 依赖：S3 清单读写与投影；S5 003 的合并检查内存结构（成功后需清除结论）。判定口径见 `requirements.md §3.13`。

---

## 1. 背景与目标

### 现状

清单 `projects` 数组顺序就是列表展示顺序（`data-model.md §2`），但用户无法调整：顺序只能是创建顺序。把一个 worktree 从一个迭代挪到另一个迭代只能手工 `git worktree move` 再手改两份清单，极易撞出重复 `worktreePath` 让清单读成 `damaged`。

### 目标

- 迭代内拖拽重排：拖动手柄 `⠿` 把项目行放到同一迭代的另一位置，后端重排 `projects` 数组并原子写清单。
- 跨迭代移动：拖到另一个迭代的卡片上，后端用 `git worktree move` 真实搬运目录，并把清单记录从源迭代迁移到目标迭代（目录名按目标迭代占用情况顺延）。
- 两种操作共用一套落点语义：「插到锚点行之前，锚点为 `null` 即追加到该迭代末尾」；前端只提交锚点 `worktreePath`，不做索引换算。
- 在真实 Tauri 窗口（WKWebView / WebView2）里拖拽可用：光标、拖影、落点指示正常。

### 非目标

- 不支持多选拖拽、不支持拖拽公共目录。
- 不支持把项目拖到已归档迭代、清单缺失或损坏的迭代。
- 不支持拖拽创建新迭代。
- 不做撤销。
- 排序不影响创建顺序、不影响任何 Git 行为，只影响列表展示与合并检查/归档评估的遍历顺序。

---

## 2. 已确认决策

1. **落点语义统一为「插到锚点之前」，`beforeWorktreePath = null` 表示追加到末尾；前端只提交锚点。**
   被否决备选：前端提交目标索引。原因：列表可能被搜索过滤、合并检查逐行更新，前端索引与清单数组索引不保证一致；锚点用规范化 `worktreePath` 在后端唯一定位，不受展示状态影响。
2. **同迭代走 `reorder_project`，跨迭代走 `move_project`，是两个命令。**
   被否决备选：一个 `move_project` 同时处理同迭代（`targetIteration == iteration`）。原因：同迭代重排不需要 Git、不需要改目录名，风险等级完全不同；分开后 `reorder` 的预检与测试都简单得多。
3. **跨迭代移动先 `git worktree move` 搬运本体，成功后再写清单：先写目标清单（插入记录），再写源清单（删除记录），各自原子写；不引入跨文件事务。**
   被否决备选 A：先写清单再搬目录。原因：搬运是最可能失败的一步（权限、锁、脏工作区），先写清单会在搬运失败时留下「清单指向不存在的路径」。
   被否决备选 B：实现两阶段提交 / 回滚。原因：`architecture.md §6.1` 只有单文件原子写，跨文件事务成本高且仍无法覆盖第二次写失败后回滚也失败的情况；改为给出明确恢复指引。
4. **目标目录名沿用 `projectId` / `projectId-N`，按目标迭代清单的占用集合顺延；占用集合包含 `removed` 与 `createFailed` 记录，以及目标迭代目录下实际存在的同名目录。**
   被否决备选：只看 `active` 记录。原因：`removed` 记录仍留在清单里且保留 `worktreePath`，同名会撞出重复 `worktreePath`，清单读成 `damaged`（`data-model.md §2` 读取健康度规则）。
5. **`worktree move` 对脏工作区只用单个 `-f`；`locked` 与 `prunable` 由预检拦截，禁止 `-f -f`。**
   被否决备选：遇 `locked` 用 `-f -f` 强制。原因：`architecture.md §5` 白名单明确禁止；lock 通常表示用户或其他工具正在使用。
6. **手柄 `⠿` 是唯一 `draggable="true"` 的元素，`<tr>` 不设 `draggable`。**
   被否决备选：整行可拖。原因：整行可拖会让 002 的单击复制与文本选择失效（`mousedown` 被拖拽劫持）。
7. **`tauri.conf.json` 的 `app.windows[].dragDropEnabled = false`。**
   原因：Tauri 默认启用系统文件拖放（Windows 下由 OLE DropTarget、macOS 下由 NSDraggingDestination 实现），会抢走页内 HTML5 拖拽事件，表现为光标变「禁止」、拖影不跟随、`drop` 不触发。本应用不使用系统文件拖入，关闭无副作用（`architecture.md §2.3`）。
8. **落点是自己原位时完全不发请求。** 原位定义：锚点等于自身，或锚点等于自身在清单中的下一条记录（两者都等价于「位置不变」）。
9. **成功后前端清掉源迭代与目标迭代的合并检查结论（内存 `Map`），然后 `listWorkspaces(false)` 快扫重载。**
   被否决备选：移动后自动触发复核或合并检查。原因：违反 `requirements.md §3.5`「复核只由按钮触发」；且移动后记录的 `worktreePath` 已变，旧结论按路径匹配会失效，清掉比保留错误结论安全。
10. **`manifestHealth ≠ valid` 的迭代不接受落点**（`dragover` 不 `preventDefault`），已归档迭代不在列表中自然不可作为落点。
11. **搜索过滤激活时手柄置灰不可拖。** 原因：过滤后相邻行不代表清单中相邻，「插到锚点之前」语义在用户视角会产生跳跃，禁用比解释更清楚。
12. **两个命令都持操作锁。**
13. **必须在真实 Tauri 窗口验证拖拽**；浏览器预览不能作为通过依据。

---

## 3. 技术方案概要

### 数据结构

引用 `data-model.md`：

- `MoveProjectRequest { iteration, worktreePath, targetIteration, beforeWorktreePath: string | null }`
- `MoveOutcome { iteration, targetIteration, projectId, worktreePath, targetWorktreePath, targetDirectory }`
- `reorder_project(iteration, worktreePath, beforeWorktreePath: string | null)` 无返回值。
- 清单 `Manifest.projects: Vec<ManifestProject>`，数组顺序即展示顺序。移动时记录的快照字段（`projectId`、`sourceRepository`、`headMode`、`branch`、`baseCommit`、`baseRef`、`createdAt`、`lifecycle`、`createResult`、`vendor`、`postSteps`、`removedAt`）原样迁移，只改 `worktreePath`；`vendor.sourcePath` 指向源仓库不变。

无新增持久化字段。

### 后端

模块：`src-tauri/src/relocate.rs`。

```rust
/// 同迭代重排：把 worktree_path 对应记录移到 before 之前（None＝末尾）
pub fn reorder(
    config: &AppConfig, iteration: &str, worktree_path: &str, before: Option<&str>,
) -> Result<(), AppError>;

/// 跨迭代移动
pub fn move_project(config: &AppConfig, req: &MoveProjectRequest) -> Result<MoveOutcome, AppError>;

/// 预检结果（内部）
struct MovePlan {
    source_manifest: Manifest, target_manifest: Manifest,
    record_index: usize,            // 源清单中的下标
    old_path: PathBuf, new_path: PathBuf, target_directory: String,
    dirty: bool,                    // 决定是否加单个 -f
    insert_at: usize,               // 目标清单插入下标
}
fn plan_move(config: &AppConfig, req: &MoveProjectRequest) -> Result<MovePlan, AppError>;
```

`reorder` 步骤：

1. 校验 `iteration`；读清单，`missing`/`damaged` → `AppError::ManifestDamaged`。
2. `paths_equal` 定位 `worktree_path` 记录，找不到 → `NotFound`；`before` 非 `None` 时同样定位，找不到 → `NotFound`；`before == worktree_path` 或 `before` 是其下一条 → 直接返回 `Ok(())`（原位）。
3. `Vec::remove` 再按 `before` 重新定位下标 `insert`（`None` → `push`）。
4. `atomic_json::write_json_atomic`。

`move_project` 预检（`plan_move`，任一不通过即返回错误，**不改动任何内容**）：

| # | 检查 | 失败错误 |
| --- | --- | --- |
| 1 | `iteration`、`targetIteration` 通过 `validate_iteration` 且不相等 | `Validation` |
| 2 | 源清单与目标清单都 `valid`（目标清单 `missing` 也拒绝：目标迭代必须已由本工具创建） | `ManifestDamaged` / `NotFound` |
| 3 | 目标清单 `archivedAt == null` | `Conflict("目标迭代已归档")` |
| 4 | 源记录存在且 `lifecycle == active` | `NotFound` / `Conflict` |
| 5 | 源记录目录名等于 `projectId` 或匹配 `projectId-N`（N ≥ 2 整数） | `Conflict("目录名不符合托管规则")` |
| 6 | `old_path` 存在、`canonicalize` 后在 `{root}/{iteration}` 内 | `Validation("pathInvalid")` |
| 7 | 源仓库 `worktree list --porcelain` 中注册了 `old_path`（`paths_equal`） | `Conflict("worktree 未在源仓库注册")` |
| 8 | 该条目无 `locked` | `Conflict("worktree 已被锁定")` |
| 9 | 该条目无 `prunable` | `Conflict("worktree 处于 prunable 状态")` |
| 10 | 目标名：`path_utils::next_directory_name(occupied, projectId)`，`occupied` = 目标清单全部记录目录名（含 `removed`/`createFailed`）∪ 目标迭代目录下实际存在的条目名 ∪ 目标迭代公共目录目标名 | — |
| 11 | `new_path = join_segments(root, targetIteration, target_directory)` 不存在 | `Conflict` |
| 12 | `before` 非 `None` 时在目标清单中可定位 | `NotFound` |
| 13 | `status --porcelain=v1 --untracked-files=all` 非空 → `dirty = true` | — |

执行：

```text
（预检）
worktree list --porcelain                                   # 源仓库，#7-#9
status --porcelain=v1 --untracked-files=all                 # 在 old_path 执行，#13
（执行）
worktree move <old_path> <new_path>                         # dirty=false
worktree move -f <old_path> <new_path>                      # dirty=true；绝不 -f -f
```

`worktree move` 失败 → `AppError::Git`（脱敏），不写任何清单。
成功后：

1. 目标清单：克隆记录，`worktreePath = new_path` 字符串，在 `insert_at` 插入，原子写。失败 → `AppError::Io("worktree 已搬到 {new_path}，但目标迭代清单写入失败：{原因}。请点击「复核状态」（该 worktree 将以 discovered 形式出现）或手工修正两份清单。")`。
2. 源清单：删除该记录，原子写。失败 → `AppError::Io("worktree 已搬到 {new_path} 且已登记到目标迭代，但源迭代清单未能删除旧记录：{原因}。请点击「复核状态」（旧记录将显示目录缺失）或手工修正源清单。")`。
3. 返回 `MoveOutcome`。

Command 适配层（`lib.rs`）：

```rust
#[tauri::command]
async fn reorder_project(state, iteration: String, worktree_path: String, before_worktree_path: Option<String>)
    -> Result<(), AppErrorObject>     // 持锁 "reorder_project"
#[tauri::command]
async fn move_project(state, request: MoveProjectRequest)
    -> Result<MoveOutcome, AppErrorObject>  // 持锁 "move_project"
```

### 前端

入口组件：`src/components/WorkspaceList.vue`（跨卡片拖拽状态）与 `src/components/IterationCard.vue`（行级事件）。

`api/tauri.ts`：`reorderProject(iteration, worktreePath, beforeWorktreePath)`、`moveProject(request)`。演示模式 reject `unsupported`。

拖拽状态（`WorkspaceList.vue` 内 `ref`，通过 props/emits 传给卡片）：

```ts
const drag = ref<{ iteration: string; worktreePath: string } | null>(null)
const dropTarget = ref<{ iteration: string; beforeWorktreePath: string | null } | null>(null)
```

事件流（按 `ui-spec.md §7.5`）：

1. 手柄 `dragstart`：`dataTransfer.setData('text/plain', worktreePath)`、`effectAllowed = 'move'`；写 `drag`。搜索框非空时手柄 `draggable=false` 且置灰。
2. 行 `dragover`：目标卡片 `manifestHealth !== 'valid'` → 不 `preventDefault`（浏览器显示禁止）；否则 `preventDefault()`，按 `event.clientY` 与行 `getBoundingClientRect()` 中线判断上/下半，计算 `beforeWorktreePath`（上半＝该行；下半＝该行的下一行 `worktreePath`，无下一行则 `null`）；写 `dropTarget`，落点行显示 2px `--primary` 上边线（末尾落点时表尾显示下边线）。
3. 卡片表格空白区 / 表尾 `dragover`：`beforeWorktreePath = null`。
4. `drop`：
   - 同迭代且原位（`before === self` 或 `before === next(self)`）→ 清状态，不发请求。
   - 同迭代 → `reorderProject(iteration, self, before)`。
   - 跨迭代 → `moveProject({ iteration, worktreePath: self, targetIteration, beforeWorktreePath: before })`。
   - 成功：删除 `mergeResults` 中 `iteration` 与 `targetIteration` 两键 → `listWorkspaces(false)` → toast（移动时文案含 `targetDirectory`）。
   - 失败：toast danger 显示后端 `message`（含恢复指引）。
5. `dragend`：清 `drag` 与 `dropTarget`。

原位判断在前端做只是为了省一次请求；后端 `reorder` 同样做原位短路，两边一致。

### 事件

无新增 Tauri 事件。

---

## 4. 状态与枚举

| 枚举 / 字段 | 取值 | 含义 |
| --- | --- | --- |
| `Lifecycle` | `active` | 唯一允许移动的记录状态 |
| | `removed` / `createFailed` | 不可移动，但在目标迭代中**占用目录名** |
| `ManifestHealth` | `valid` | 唯一允许作为源与落点的清单状态 |
| | `missing` / `damaged` | 拒绝作为源与落点 |
| `RiskCode`（复用语义） | `worktreeLocked` / `prunable` / `pathInvalid` | 预检 #6/#8/#9 的错误 message 引用这些名称便于用户对照移除对话框文案 |
| `ErrorCode` | `validation` / `notFound` / `conflict` / `manifestDamaged` / `git` / `io` / `busy` | 见 §3 各步 |
| `Validity`（移动后复核） | `discovered` | 第一次写失败后的目标侧表现 |
| | `missingDirectory` | 第二次写失败后的源侧表现 |

---

## 5. 测试要点

对应 `implementation-plan.md` S8。

Rust 单测（纯函数）：

- `reorder`：三条记录 `a,b,c`；`before=null` 移 `a` → `b,c,a`；`before=a` 移 `c` → `c,a,b`；`before=b` 移 `a` → 不变且文件 mtime 不变（原位短路）；`before=c` 移 `b` → 不变（下一条即原位）；`before` 不存在 → `NotFound`。
- 目录名规则：`p` / `p-2` / `p-10` 通过；`p-1`、`p-0`、`p-x`、`px` 拒绝。
- `next_directory_name`：目标清单有 `active p`、`removed p-2`、`createFailed p-3` → 返回 `p-4`；目标目录下存在未托管 `p-4` 目录 → 返回 `p-5`。
- Git 参数向量：`["worktree","move","<old>","<new>"]` 与 `["worktree","move","-f","<old>","<new>"]`；断言任何路径下都不产生两个 `-f`。

Rust 集成测试（`tempfile` + 裸仓库；无 git 则打印原因并 return）：

- 干净 worktree 跨迭代移动：目录真实搬走、源仓库 `worktree list` 显示新路径、目标清单含记录且 `worktreePath` 为新路径、源清单无该记录、两份清单 `worktreePath` 无重复且可读。
- 目标清单已有 `removed` 同名记录 → 落到 `p-2`。
- 脏 worktree（有 untracked 文件）→ 使用单个 `-f` 成功搬运，文件仍在。
- `git worktree lock` 后移动 → `Conflict`，目录未动，清单未动。
- `prunable`（手工删除 worktree 目录后按清单路径移动）→ 预检 #6 或 #9 拒绝。
- 目标迭代 `archivedAt` 非空 → `Conflict`。
- 目标清单 `damaged`（写入非法 JSON）→ `ManifestDamaged`，目录未动。
- 操作锁：持锁期间调用 → `busy`。
- 第二次写失败模拟（源清单所在目录在搬运后改为只读）→ 返回 `Io` 且 message 含「复核状态」；目标清单已写入。

前端 SSR 测试：

- `IterationCard` 渲染时 `<tr>` 不含 `draggable`，手柄元素含 `draggable="true"`。
- 传入 `searchActive: true` 时手柄含禁用 class。

真实 Tauri 窗口手工验证（必做）：

- 拖动手柄时光标不是「禁止」、拖影跟随鼠标；若异常，先确认 `tauri.conf.json` 的 `dragDropEnabled` 为 `false`。
- 同卡片拖到另一行上半 / 下半，蓝色指示线位置正确；释放后顺序改变，重启应用仍保持。
- 拖到自身原位（上一行下半或自身上半）释放 → DevTools Network/日志无 `reorder_project` 调用。
- 拖到另一张卡片 → 目录搬走，两张卡片刷新，两侧合并检查列回到「未复核」。
- 拖到清单损坏的卡片 → 浏览器禁止光标，释放无效果。
- 搜索框有内容时手柄置灰不可拖，点击路径格仍可复制、文本仍可选择。

---

## 6. 已知局限

- 跨迭代移动没有跨文件事务：`worktree move` 成功后若目标清单写入失败，磁盘目录已在新位置、两份清单都没有它（复核后显示为 discovered）；若源清单写入失败，两份清单同时含该记录但路径不同（源侧显示目录缺失）。两种情况都只给恢复指引，不自动回滚。
- 已归档迭代不在列表中，不能作为落点；清单缺失的迭代目录也不能作为落点（必须先用本工具在该迭代创建过至少一次）。
- 排序只影响列表展示以及合并检查 / 归档评估的遍历顺序，不影响任何 Git 行为。
- 不支持多选拖拽、不支持键盘排序（无 `ArrowUp/Down` 快捷键）。
- 移动不迁移公共目录与 vendor 的「来源」语义：`vendor.copiedByTool` 等字段原样保留，但目标迭代的公共目录（如 `fd-common`）可能与源迭代内容不同，工具不做提示。
- `dirty` 判定在预检时执行，与 `worktree move -f` 之间存在竞态；若期间工作区从干净变脏，`move` 会因无 `-f` 失败，用户重试即可。
- 搜索过滤期间禁止拖拽，用户需先清空搜索。

---

## 7. 实现记录

实现方填写：

- 完成日期：
- 偏离项：
- 原因：
- 涉及文件：

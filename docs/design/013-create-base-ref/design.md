# 013 · 创建基分支（统一 / 逐项目基分支与远端分支列举）

> 设计文档是当次决策的历史记录。设计确认后不回改正文；实现偏离时在 §7「实现记录」追加说明。
> 依赖：S3 创建流程（`workspace.rs`、`CreateWorkspace.vue`）。判定口径见 `requirements.md §3.2`、`§3.12`。

---

## 1. 背景与目标

### 现状

MVP 创建流程按 `requirements.md §3.2` 对每个项目执行一次 `fetch <remote> <branch>` 并从 `FETCH_HEAD` 创建 worktree。基分支的来源在 MVP 阶段是创建请求里的 `baseRef` 字符串，但没有统一的归一化规则，也没有给用户提供候选列表：用户必须手工敲出完整的 `origin/xxx`，输错只能在 fetch 失败后才知道。

### 目标

- 创建页提供「统一基分支」输入与逐项目「基分支」输入，都支持手输 + `<datalist>` 候选。
- 候选来自源仓库本地 `refs/remotes`（默认，零网络），用户点 `⟳` 才通过 `ls-remote` 联网刷新；失败降级为本地候选并给出可见的报错提示，不阻断创建。
- 后端统一归一化规则（`base_ref.rs::normalize`），三种输入形态（空串 / `remote/branch` / 裸分支名）得到确定的 `<remote>/<branch>`；无法归一化时该项目失败（`createFailed`），并且失败记录也保留归一化尝试的结果。
- 列表「分支 / HEAD」列下方显示「基于 {baseRef}」。

### 非目标

- 不做分支名模糊匹配 / 自动补全算法（`<datalist>` 的原生前缀过滤即可）。
- 不缓存远端列举结果到磁盘或跨页面。
- 不改变 fetch 策略（仍是每项目一次 `fetch <remote> <branch>`）。
- 不支持 tag、commit hash 作为基分支。

---

## 2. 已确认决策

1. **归一化在后端 `base_ref.rs` 完成，前端 `utils/base-ref.ts` 只做展示预览。**
   被否决备选：前端归一化后把 `origin/xxx` 提交给后端。原因：`architecture.md §4.3` 前端不可信；remote 列表只有后端能真实读到。
2. **归一化三态（`requirements.md §3.12`）**：空串 → `origin/master`；`<remote>/<branch>` 首段与 `git remote` 输出中某个 remote 同名 → 按该 remote 解析，原样保留；裸分支名 → 优先 `origin`，没有 `origin` 且只有一个 remote 时用该 remote；其余情况（remote 不存在、零个或多个 remote 且无 `origin`、分支名不通过 `check-ref-format --branch`）→ 该项目失败。
   被否决备选：裸分支名在多 remote 且无 `origin` 时取第一个 remote。原因：`git remote` 输出顺序不稳定，隐式选择会造成静默错误。
3. **打开创建页不发起任何远端连接。** 进入页面时只对已配置项目调用 `list_remote_branches(projectId, includeRemote=false)`（读本地 `refs/remotes`）。
   被否决备选：进入页面自动 `ls-remote` 全部项目。原因：10+ 仓库 × 15 秒超时在断网时会让页面卡顿数分钟；且违反「按需拉取」要求。
4. **`includeRemote=true` 对该项目的每个 remote 各执行一次 `ls-remote --heads <remote>`，15 秒超时，`GIT_TERMINAL_PROMPT=0`。** 任一 remote 失败不抛错：`RemoteBranches.source` 仍为 `local`（只要有一个 remote 失败即视为降级），`branches` 用本地候选，`warning` 放脱敏后的原始 git/ssh 报错。
   被否决备选：失败时抛 `AppError::Git`。原因：远端拉取只是候选增强，失败不应打断创建流程；错误信息通过 tooltip 展示即可。
5. **整页「⟳ 全部」按钮对已勾选项目**串行**逐个调用 `list_remote_branches(id, true)`。**
   被否决备选：`Promise.all` 并发。原因：多个 ssh 连接并发会同时弹 agent / 密钥提示，且会放大 `GIT_TERMINAL_PROMPT=0` 导致的失败；串行下单个失败可逐项显示。
6. **`list_remote_branches` 不持操作锁**（`architecture.md §7.1`），只读、可与其他操作并行。
7. **单向联动**：修改统一基分支 → 覆盖所有已勾选行的基分支；新勾选行继承当前统一值；单行修改不反写统一值；取消勾选再勾选重新继承。与统一分支名的规则一致（`requirements.md §2.1` 第 4 条）。
8. **`createFailed` 记录保留归一化结果**：归一化成功但后续 fetch 失败 → `baseRef` 为归一化值；归一化本身失败 → `baseRef` 为用户原始输入去首尾空白后的字符串（空串则为 `origin/master`），`createResult.message` 说明失败原因。
   被否决备选：失败记录 `baseRef` 置空。原因：`data-model.md §2` 约束 `baseRef` 为 `string` 非空，且用户复盘失败原因时需要看到当时用的基分支。
9. **列表「基于 XXX」直接显示清单 `baseRef`**，discovered 行无记录时不显示该行。

---

## 3. 技术方案概要

### 数据结构

引用 `data-model.md`：

- `CreateRequest.unifiedBaseRef: string`、`CreateProjectRequest.baseRef: string`（空串＝`origin/master`）。前端提交的是用户原始输入（trim 后），后端归一化。
- `ManifestProject.baseRef: string`（归一化结果）。
- `WorkspaceProject.baseRef: string | null`（投影，discovered 为 `null`）。
- `RemoteBranches { projectId, remotes: string[], branches: string[], source: "local" | "remote", warning: string | null }`。`branches` 形如 `origin/feature/x`，去重后按字典序排序。

无新增持久化字段。

### 后端

模块：`src-tauri/src/base_ref.rs`。

```rust
/// 归一化结果
pub struct NormalizedBaseRef { pub remote: String, pub branch: String }
impl NormalizedBaseRef { pub fn display(&self) -> String /* "{remote}/{branch}" */ }

/// 纯函数：根据 remote 列表归一化用户输入；不访问文件系统与 Git
pub fn normalize(remotes: &[String], input: &str) -> Result<NormalizedBaseRef, AppError>;

/// 分支名本地规则（非空、无空白、无 `..`、不以 `/` 开头结尾、无控制字符）；
/// 调用方在 workspace.rs 中再追加 `git check-ref-format --branch`
fn validate_branch_shape(branch: &str) -> Result<(), AppError>;

/// 列举候选
pub fn list_remote_branches(
    config: &AppConfig, project_id: &str, include_remote: bool,
) -> Result<RemoteBranches, AppError>;
```

`normalize` 判定顺序：

1. `input.trim()` 为空 → `origin/master`（**不**检查 `origin` 是否存在，与 `requirements.md §3.12` 一致；fetch 阶段若 `origin` 不存在自然失败）。
2. 含 `/`：以第一个 `/` 拆成 `(head, rest)`；`remotes` 含 `head` 且 `rest` 非空 → `{head}/{rest}`；否则视为裸分支名进入第 3 步（分支名本身可以含 `/`，如 `feature/x`）。
3. 裸分支名：`remotes` 含 `origin` → `origin/{input}`；否则 `remotes.len() == 1` → `{remotes[0]}/{input}`；否则 `AppError::Validation("无法确定基分支 {input} 所属的 remote：仓库没有 origin 且存在 N 个 remote")`。
4. 对得到的 `branch` 调 `validate_branch_shape`。

Git 调用序列（`workspace.rs` 创建单个项目时，与 `requirements.md §3.2` 合并）：

```text
remote                                    # 取 remotes，供 normalize
check-ref-format --branch <branch>        # 对归一化后的 branch 段；失败 → createFailed
fetch <remote> <branch>
rev-parse --verify FETCH_HEAD^{commit}
...（后续 worktree add 等不变）
```

`list_remote_branches` 调用序列：

```text
remote                                                  # 总是执行
for-each-ref --format=%(refname) refs/remotes           # includeRemote=false 或作为降级候选
ls-remote --heads <remote>                              # includeRemote=true 时对每个 remote 各一次
                                                        # 15 秒超时，GIT_TERMINAL_PROMPT=0
```

`refs/remotes/origin/HEAD` 这类符号引用从候选中剔除。`ls-remote` 输出每行 `<sha>\trefs/heads/<name>`，映射为 `<remote>/<name>`；成功时 `branches` = 远端结果 ∪ 本地缓存（并集，去重排序），`source = "remote"`。

超时实现：`std::process::Command::spawn` + 循环 `try_wait` 至 15 秒，超时 `kill()` 并返回 `AppError::Git("git ls-remote --heads <remote> 超过 15 秒未返回")`，经 `error.rs` 脱敏后放入 `warning`。

Command 适配层（`lib.rs`）：

```rust
#[tauri::command]
async fn list_remote_branches(state: State<'_, AppState>, project_id: String, include_remote: bool)
    -> Result<RemoteBranches, AppErrorObject>
```

在 `spawn_blocking` 中执行；不获取操作锁。

### 前端

入口组件：`src/components/CreateWorkspace.vue`。

`api/tauri.ts`：`listRemoteBranches(projectId: string, includeRemote: boolean): Promise<RemoteBranches>`。演示模式返回假候选（`source: "local"`）。

状态存放（组件内 `ref`，离开创建页即丢弃）：

```ts
const unifiedBaseRef = ref('')                                   // 统一基分支原始输入
const rows = ref<Map<string, { checked: boolean; branch: string; baseRef: string }>>()
const candidates = ref<Map<string, RemoteBranches>>()            // projectId → 候选
const refreshing = ref<Set<string>>()                            // 正在 ls-remote 的项目
```

UI：

- 右侧栅格「统一基分支」：`<input list="unified-base-ref">` + `<datalist id="unified-base-ref">`，候选为所有已勾选项目 `branches` 的并集；旁边 `⟳ 全部` 按钮（串行刷新已勾选项目）。
- 项目表「基分支」列：每行 `<input list="base-ref-{projectId}">` + 对应 `<datalist>`；旁 `⟳` 单项刷新按钮，刷新中禁用并显示 loading；`warning` 非空时显示 warning 图标（`⚠` 文字符号），`title` 为 `warning` 全文。
- 输入框 `placeholder="origin/master"`，下方小字用 `utils/base-ref.ts::previewNormalized(remotes, input)` 显示「将解析为：origin/xxx」或「无法确定 remote」；这只是预览，最终以后端为准。
- 进入页面：对所有已配置项目调用 `listRemoteBranches(id, false)`（并发即可，本地读取无网络）。

列表页 `IterationCard.vue`：「分支 / HEAD」格内 `branchDisplay` 下方渲染 `<small class="base-ref">基于 {{ project.baseRef }}</small>`，`baseRef === null` 时不渲染。

### 事件

无新增事件。创建阶段沿用 `create-progress`，`fetching` 阶段 `message` 为实际命令 `git fetch <remote> <branch>`（归一化后的值）。

---

## 4. 状态与枚举

| 枚举 / 字段 | 取值 | 含义 |
| --- | --- | --- |
| `RemoteBranches.source` | `local` | 候选仅来自本地 `refs/remotes`（未联网或联网失败降级） |
| | `remote` | 本次 `ls-remote` 全部 remote 成功，候选为远端 ∪ 本地 |
| `Lifecycle` | `createFailed` | 归一化失败或 fetch 失败的记录；`baseRef` 按 §2.8 保留 |
| `CreateStatus` | `failed` | 同上，`createResult.message` 含原因 |
| `CreatePhase` | `fetching` | 进度事件中 `message` 为 `git fetch <remote> <branch>` |
| `ErrorCode` | `validation` | 归一化失败 |
| | `git` | `ls-remote` / `fetch` 失败（已脱敏） |

---

## 5. 测试要点

对应 `implementation-plan.md` S8。

Rust 单测（`base_ref.rs`，纯函数，无 Git）：

- `normalize(["origin"], "")` → `origin/master`；`normalize([], "")` 亦为 `origin/master`。
- `normalize(["origin","upstream"], "upstream/release")` → `upstream/release`。
- `normalize(["origin"], "feature/x")` → `origin/feature/x`（首段 `feature` 不是 remote，整体当裸分支名）。
- `normalize(["gitea"], "release")` → `gitea/release`（无 origin、单 remote）。
- `normalize(["a","b"], "release")` → `Validation` 错误。
- `normalize(["origin"], "origin/")` → 错误（rest 为空）。
- `normalize(["origin"], "bad..name")`、`" spaced"`（trim 后合法则通过）、含 `\u{1}` → 错误。
- `ls-remote` 输出解析：`<sha>\trefs/heads/feature/x` → `origin/feature/x`；`refs/remotes/origin/HEAD` 被剔除；并集去重排序。
- Git 参数向量：`["ls-remote","--heads","origin"]`、`["for-each-ref","--format=%(refname)","refs/remotes"]`、`["remote"]` 逐条比对。

Rust 集成测试（`tempfile` + 裸仓库；无 git 则打印原因并 return）：

- `includeRemote=false` 不访问远端：把裸仓库目录改名后调用仍成功，`source=local`。
- `includeRemote=true` 且远端可达：`source=remote`，`branches` 含远端新建分支。
- 远端不可达（remote URL 指向不存在目录）：不报错，`source=local`，`warning` 非空且不含凭证（URL 里塞 `user:token@` 验证脱敏）。
- 创建时 `baseRef="upstream/release"` 且仓库无 `upstream` → 该项目 `createFailed`，清单 `baseRef` 为 `upstream/release`，`message` 说明 remote 不存在；其他项目正常。
- 创建时 `baseRef=""` → 清单 `baseRef="origin/master"`，worktree HEAD 等于远端 master。

前端 SSR 测试：

- `utils/base-ref.ts::previewNormalized` 三态与错误文案。
- `CreateWorkspace` 渲染含 `<datalist id="unified-base-ref">`。

真实窗口手工验证：

- 进入创建页时用抓包 / 断网确认无远端连接（最简单：断网进入页面不卡顿，候选仍显示本地 `refs/remotes`）。
- 点单项 `⟳` 联网成功后候选增加；把 remote URL 改坏后点 `⟳` → 图标变 `⚠`，`title` 显示报错，仍可手输并创建。
- 修改统一基分支 → 已勾选行全部同步；改一行后再改统一值 → 该行被覆盖；改一行不影响统一值输入框。
- `⟳ 全部` 时观察逐个项目依次进入 loading（串行）。
- 列表页看到「基于 origin/xxx」。

---

## 6. 已知局限

- `ls-remote` 超时固定 15 秒，不可配置；慢网络下会把可用远端误判为失败并降级（仍可手输）。
- 远端列举结果只存于创建页组件内存，切页即丢；不做磁盘缓存，也不跨会话保留。
- 不做分支名模糊匹配与拼写纠错，`<datalist>` 只有浏览器原生前缀过滤；WKWebView 的 `<datalist>` 下拉样式不可定制。
- 空串默认 `origin/master` 不检查 `origin` 是否存在，错误延迟到 fetch 阶段才暴露。
- 裸分支名含 `/` 且首段恰好与某个 remote 同名时（如 remote 叫 `feature`，分支叫 `feature/x`），会被解析成 remote `feature` + 分支 `x`；用户需显式写 `origin/feature/x`。
- `ls-remote` 需要凭证时因 `GIT_TERMINAL_PROMPT=0` 直接失败；本应用不做凭证管理（`requirements.md §2.3`）。

---

## 7. 实现记录

- 完成日期：2026-09-30（`normalize` 于 S3 落地，候选列举 / 前端在 S8）
- 偏离项：两点。① `normalize` 在 S3 就需要（创建流程要写归一化 `baseRef`），因此早于 S8 落地；
  ② 归一化后额外对 `branch` 段执行 `validation::validate_branch_name`（设计 §3.2 第 4 步的 `validate_branch_shape`）。
- 原因：S3 创建流程依赖归一化结果；分支形状校验复用既有跨平台规则，避免两份实现。
- 涉及文件：`src-tauri/src/base_ref.rs`、`src-tauri/src/workspace.rs`、`src-tauri/src/lib.rs`、
  `src/utils/base-ref.ts`（`previewNormalized`）、`src/components/CreateWorkspace.vue`、`src-tauri/tests/mvp_flow.rs`、`scripts/test-frontend.mjs`

# Worktree Manager · 数据模型

> 系统当前态的单一事实来源之一。`models.rs` 与 `src/types.ts` 必须与本文逐字段、逐枚举对应；任何新增字段先改本文再改代码。
> JSON 一律 `camelCase`；两份持久化模型都带 `schemaVersion`，当前值 `1`。时间戳一律 RFC 3339 UTC 字符串（`chrono::DateTime<Utc>`）。

---

## 1. 全局配置 `config.json`

位置：`platform::config_dir()/config.json`（macOS：`~/Library/Application Support/WorktreeManager/config.json`）。

```json
{
  "schemaVersion": 1,
  "workspaceRoot": null,
  "sharedDirectories": [],
  "projects": [],
  "recentIterations": []
}
```

填写后的示例：

```json
{
  "schemaVersion": 1,
  "workspaceRoot": "/Users/me/Work/workspace",
  "sharedDirectories": [
    { "sourcePath": "/Users/me/Work/fd-common", "targetDirectory": "fd-common" }
  ],
  "projects": [
    { "id": "api3", "repositoryPath": "/Users/me/Work/php-project/api3", "projectType": "php", "vendorAvailable": true }
  ],
  "recentIterations": ["7.3.0"]
}
```

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `workspaceRoot` | `string \| null` | `null`＝未设置。非空时必须是绝对路径；允许尚不存在（校验最近已存在祖先）。保存时去首尾空白，空串规范化为 `null` |
| `sharedDirectories[].sourcePath` | `string` | 绝对路径；保存时 `canonicalize`；创建前要求是可读普通目录（非链接） |
| `sharedDirectories[].targetDirectory` | `string` | 安全单层目录名（见 validation 规则）；大小写不敏感唯一 |
| `projects[].id` | `string` | 非空、安全单层目录名、大小写不敏感唯一、不能与任一 `targetDirectory` 大小写不敏感相等 |
| `projects[].repositoryPath` | `string` | 已存在的真实 Git 根目录（`rev-parse --show-toplevel` 的规范化结果） |
| `projects[].projectType` | `ProjectType` | `php \| go \| other \| unknown` |
| `projects[].vendorAvailable` | `boolean` | 添加/保存时的本地快照；创建时重新检查真实目录 |
| `recentIterations` | `string[]` | 创建页历史候选，最多 10 条，最近在前，去重；只存通过校验的迭代号 |

**旧格式迁移**（`config.rs::migrate`）：顶层 `fdCommonSource: string` → 追加一条 `{ sourcePath, targetDirectory: "fd-common" }`；`goCommonSource` 直接丢弃；旧 `sharedDirectories[]` 里 `enabled === false` 的规则丢弃、`enabled === true` 或缺省的保留（去掉 `enabled` 字段）。迁移只在内存中进行，用户保存时才写回新格式。

`ProjectType` 判定（`config.rs::detect_project_type`）：根目录存在 `composer.json` → `php`；存在 `go.mod` → `go`；两者都无但是 Git 根 → `other`；无法读取 → `unknown`。`vendorAvailable` = `php` 且 `vendor/` 是目录。

---

## 2. 迭代清单 `.worktree-manager.json`

位置：`{workspaceRoot}/{iteration}/.worktree-manager.json`。

```json
{
  "schemaVersion": 1,
  "iteration": "7.3.0",
  "createdAt": "2026-07-30T10:00:00Z",
  "note": null,
  "hiddenAt": null,
  "archivedAt": null,
  "sharedDirectories": [
    {
      "ruleId": "fd-common",
      "sourcePath": "/Users/me/Work/fd-common",
      "targetDirectory": "fd-common",
      "targetPath": "/Users/me/Work/workspace/7.3.0/fd-common",
      "status": "copied",
      "message": null
    }
  ],
  "projects": [
    {
      "projectId": "api3",
      "sourceRepository": "/Users/me/Work/php-project/api3",
      "worktreePath": "/Users/me/Work/workspace/7.3.0/api3",
      "headMode": "branch",
      "branch": "feature/example",
      "baseCommit": "0123456789abcdef0123456789abcdef01234567",
      "baseRef": "origin/release",
      "createdAt": "2026-07-30T10:01:00Z",
      "lifecycle": "active",
      "createResult": { "status": "created", "message": null },
      "vendor": {
        "sourcePath": "/Users/me/Work/php-project/api3/vendor",
        "lockHash": "sha256 hex",
        "status": "copied",
        "copiedByTool": true,
        "message": null
      },
      "postSteps": [ { "name": "vendor", "status": "success", "message": null } ],
      "removedAt": null
    }
  ]
}
```

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `iteration` | `string` | 与目录名一致 |
| `note` | `string \| null` | ≤50 字符（按 Unicode 标量计）、单行、去首尾空白；空串＝清除 |
| `hiddenAt` / `archivedAt` | `string \| null` | 时间戳；`archivedAt` 非空的迭代**不出现在列表** |
| `sharedDirectories[].ruleId` | `string` | 等于 `targetDirectory`（规则的稳定标识） |
| `sharedDirectories[].status` | `SharedDirStatus` | `pending \| reused \| copied \| failed`；读旧清单时接受 `notRequired` |
| `projects[].projectId` | `string` | 允许同一 `projectId` 多条 |
| `projects[].worktreePath` | `string` | 同一迭代内规范化后唯一；目录名为 `projectId`、`projectId-2`、`projectId-3`… |
| `projects[].headMode` | `branch \| detached` | |
| `projects[].branch` | `string \| null` | `detached` 时为 `null`；011 改名同步时会被原子回写 |
| `projects[].baseCommit` | `string \| null` | 40 位 hex；`createFailed` 时可为 `null` |
| `projects[].baseRef` | `string` | 归一化形如 `origin/release`；`createFailed` 也保留归一化结果 |
| `projects[].lifecycle` | `active \| removed \| createFailed` | |
| `projects[].createResult` | `{ status: created \| alreadyExists \| failed, message }` | |
| `projects[].vendor` | `VendorRecord \| null` | 非 PHP 项目为 `{ status: "notPhp", ... }` 或 `null`，实现取 `notPhp` 记录以便列表显示「无需复制」 |
| `projects[].postSteps[]` | `{ name, status: success \| skipped \| failed, message }` | 目前只有 `vendor` 一项 |
| `projects[].removedAt` | `string \| null` | |

**顺序约束**：`projects` 数组顺序就是列表展示顺序（014）。
**失败记录约束**：`createFailed` 的项目允许目录尚未创建、`baseCommit` 为 `null`，但必须保存预期 `worktreePath`、请求的 `branch`/`headMode`/`baseRef` 与 `createResult.message`。
**读取健康度**：文件缺失 → `missing`；JSON 解析失败、`schemaVersion` 不是 1、`iteration` 与目录名不一致、`worktreePath` 重复 → `damaged`（只报错，不改写）。

---

## 3. 枚举总表（`models.rs` 必须逐个对照）

| Rust 枚举 | 取值（serde 字符串） |
| --- | --- |
| `ProjectType` | `php \| go \| other \| unknown` |
| `SharedDirStatus` | `pending \| reused \| copied \| failed \| notRequired`（`notRequired` 仅读兼容，新写入禁止） |
| `HeadMode` | `branch \| detached` |
| `Lifecycle` | `active \| removed \| createFailed` |
| `CreateStatus` | `created \| alreadyExists \| failed` |
| `VendorStatus` | `copied \| notPhp \| sourceMissing \| lockMissing \| lockMismatch \| targetExists \| copyFailed` |
| `PostStepStatus` | `success \| skipped \| failed` |
| `Validity` | `valid \| missingDirectory \| notRegistered \| headMismatch \| sourceMissing \| removed \| discovered \| unknown` |
| `ManifestHealth` | `valid \| missing \| damaged` |
| `MergeCellStatus` | `merged \| contained \| unmerged \| targetMissing \| branchMissing \| notCheckable \| error` |
| `RiskSeverity` | `warning \| blocking` |
| `RiskCode` | `trackedChanges \| untrackedFiles \| unpushedCommits \| detachedCommits \| worktreeLocked \| pathInvalid \| manifestInvalid \| prunable` |
| `CreatePhase` | `queued \| fetching \| creating \| vendor \| completed \| failed` |
| `MergeCheckPhase` | `fetching \| checking \| record` |
| `ErrorCode` | `validation \| busy \| notFound \| conflict \| manifestDamaged \| git \| io \| unsupported` |

`Validity` 语义：

| 值 | 含义 |
| --- | --- |
| `valid` | 目录存在、在源仓库 `worktree list` 中注册、HEAD/分支与清单一致（或按 011 判定为本地改名）。`detached` 记录：仍处于 detached 状态即为 `valid`（HEAD 上有新提交也算），被检出到某个分支才判 `headMismatch` |
| `missingDirectory` | 清单记录 `active` 但目录不存在 |
| `notRegistered` | 目录存在但源仓库 `worktree list` 里没有该路径 |
| `headMismatch` | 注册了，但检出的分支与清单不一致且清单分支仍存在（切分支/串台） |
| `sourceMissing` | 源仓库路径不存在或不是 Git 根 |
| `removed` | `lifecycle = removed` |
| `discovered` | 磁盘存在、清单无记录的 worktree（仅复核档出现）。来源：已配置源仓库 `worktree list --porcelain` 中位于该迭代目录下、且清单无记录的条目；迭代目录下含 `.git` 文件但不属于任何已配置仓库的子目录也列为 `discovered`，但 `sourceRepository` 为空串、`removable = false` |
| `unknown` | 快扫档下目录存在、未复核 |

---

## 4. 请求与结果结构

```text
CreateRequest      iteration, note?: string | null (缺省＝不改动), unifiedBranch: string | null,
                   unifiedBaseRef: string, projects[]: CreateProjectRequest
```

`note` 三态约定：Rust 侧类型为 `Option<Option<String>>`，配合 `#[serde(default, deserialize_with = "deserialize_optional_field")]`：字段缺省 → `None`（不改动）；`null` → `Some(None)`（清除）；字符串 → `Some(Some(s))`（设置）。TS 侧 `note?: string | null`，前端不传即缺省，**禁止用 `note: ""` 表达不改动**。

```text
CreateProjectRequest projectId, branch: string | null (null＝detached), baseRef: string (空串＝origin/master)
CreateBatchResult  iteration, iterationPath, sharedDirectories[]: SharedDirRecord,
                   projects[]: CreateProjectResult{ projectId, worktreePath, status: CreateStatus,
                   message, vendorStatus: VendorStatus | null }, aborted: boolean, abortReason: string | null

ProjectResolution  inputPath, repositoryPath: string | null, suggestedId: string | null,
                   projectType, vendorAvailable, error: string | null
RemoteBranches     projectId, remotes: string[], branches: string[] (形如 origin/xxx，去重排序),
                   source: local | remote, warning: string | null (ls-remote 失败原文，已脱敏)

RemoveRequest      iteration, projectId, worktreePath, confirmation, removeCopiedVendor: boolean
RemovalAssessment  iteration, projectId, worktreePath, confirmationText (= "{iteration}/{目录名}"),
                   allowed: boolean, vendorOnlyCleanupAvailable: boolean,
                   risks[]: { code: RiskCode, severity, message, paths: string[] }

MergeCheckResult   iteration, checkedAt, records[]: MergeRecordResult
MergeRecordResult  projectId, branchDisplay, worktreePath, lifecycle,
                   develop: MergeCellResult, master: MergeCellResult,
                   hasChanges: boolean | null, dirty: boolean | null
MergeCellResult    status: MergeCellStatus, unmergedCommits: string[] (形如 "<短hash> <subject>"),
                   errorMessage: string | null, stale: boolean, dirty: boolean

ArchiveRequest     iteration, confirmation, force: boolean
ArchiveAssessment  iteration, confirmationText (= iteration), checkedAt, clean: boolean,
                   records[]: { projectId, branchDisplay, worktreePath, develop, master,
                                dirty: boolean, clean: boolean, blockers: string[] }
ArchiveOutcome     iteration, removedCount, failed[]: { projectId, worktreePath, message }, archived: boolean

MoveProjectRequest iteration, worktreePath, targetIteration, beforeWorktreePath: string | null
MoveOutcome        iteration, targetIteration, projectId, worktreePath, targetWorktreePath, targetDirectory

CreateProgress     projectId, index, total, phase: CreatePhase, message
MergeCheckProgress iteration, projectId, worktreePath, index, total, phase: MergeCheckPhase,
                   message, record: MergeRecordResult | null
```

---

## 5. 投影结构（列表页）

```text
WorkspaceGroup     iteration, iterationPath, manifestHealth, manifestMessage: string | null,
                   openable: boolean, note: string | null, hiddenAt: string | null,
                   sharedDirectories[]: SharedDirRecord, projects[]: WorkspaceProject
WorkspaceProject   projectId, branchDisplay: string, baseCommit: string | null, baseRef: string | null,
                   sourceRepository: string, worktreePath: string, vendorStatus: VendorStatus | null,
                   createdAt: string | null, validity: Validity, openable: boolean, removable: boolean,
                   renamedFrom: string | null, dirty: boolean | null, hasChanges: boolean | null,
                   lifecycle: Lifecycle | null (discovered 为 null)
```

- `branchDisplay`：`branch` 模式显示分支名；`detached` 显示 `detached @ <短hash>`；discovered 显示 live 名或 `detached @ <短hash>`。
- `openable`：目录存在。
- `removable`：`validity ∈ {valid, headMismatch}` 且 `lifecycle ≠ removed`，或 `validity = discovered` 且 `sourceRepository` 非空；仅表示「可以打开移除对话框」，真正能否移除以 `assess_removal` / `assess_discovered_removal` 为准（discovered 行走后者，且 `vendorOnlyCleanupAvailable` 恒为 `false`，因为无清单记录无法证明 vendor 由工具复制；`remove_discovered_worktree.removeCopiedVendor` 必须为 `false`，否则返回校验错误）。
- `renamedFrom`：011 改名同步时非空。复核完成后前端统计 `renamedFrom` 非空的行数 N，N>0 时 toast「已同步 N 个分支重命名」；命令签名不变。
- `dirty` / `hasChanges`：快扫为 `null`；复核后为布尔；若前端持有该迭代的合并检查结果，则用合并检查的值覆盖（006）。合并检查结果的作废时机：创建 / 移除 / 归档 / 跨迭代移动后按迭代作废；同迭代排序与复核不作废。
- `manifestHealth ≠ valid` 的迭代：`projects` 为空、`openable` 取目录存在与否、卡片按钮组中「备注 / 隐藏 / 归档 / 检查合并」禁用、不可作为拖拽落点。
- `archivedAt` 非空的迭代根本不进入返回数组。

快扫（`reconcile=false`）：只枚举 `workspaceRoot` 的一级子目录，读清单，`validity` 取 `removed` / `missingDirectory`（目录不存在）/ `unknown`（目录存在），不跑 Git，不做 discovered 扫描。
复核（`reconcile=true`）：对每条 `active` 记录跑 `worktree list --porcelain`（按源仓库缓存，每仓库一次）、`rev-parse HEAD`、`symbolic-ref`、`status --porcelain`，再对迭代目录下每个含 `.git` 文件的子目录做 discovered 扫描；011 改名回写。

---

## 6. TypeScript 类型（`src/types.ts`）

与上述结构一一对应，命名规则：Rust 结构名即 TS 接口名；枚举用字符串字面量联合类型；`Option<T>` → `T | null`；`Vec<T>` → `T[]`。`AppErrorObject = { code: ErrorCode; message: string }`。

`src/types.ts` 不含任何运行时代码（只有 `export type` / `export interface`），以满足 `verbatimModuleSyntax`。

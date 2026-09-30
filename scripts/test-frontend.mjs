// 前端 SSR 冒烟测试（AGENTS.md §11）：
// 只用 Vite createServer + ssrLoadModule + createSSRApp + renderToString + node:assert/strict，
// 不引入任何测试框架。测试结束关闭 Vite server 并按结果退出。
import assert from "node:assert/strict";
import { createServer } from "vite";
import { createSSRApp } from "vue";
import { renderToString } from "vue/server-renderer";

const server = await createServer({
  server: { middlewareMode: true },
  appType: "custom",
  logLevel: "error",
});

let passed = 0;
let failed = 0;

async function check(name, fn) {
  try {
    await fn();
    passed += 1;
    console.log(`ok ${passed + failed} - ${name}`);
  
} catch (error) {
    failed += 1;
    console.error(`not ok ${passed + failed} - ${name}`);
    console.error(`  ${error && error.message ? error.message : String(error)}`);
  }
}

async function render(componentPath, props) {
  const module = await server.ssrLoadModule(componentPath);
  const app = createSSRApp(module.default, props);
  return renderToString(app);
}

try {
  const status = await server.ssrLoadModule("/src/utils/status.ts");
  const merge = await server.ssrLoadModule("/src/utils/merge.ts");
  const note = await server.ssrLoadModule("/src/utils/note.ts");
  const baseRef = await server.ssrLoadModule("/src/utils/base-ref.ts");
  const storage = await server.ssrLoadModule("/src/utils/storage.ts");
  const columns = await server.ssrLoadModule("/src/utils/columns.ts");
  const demo = await server.ssrLoadModule("/src/api/demo-data.ts");

  await check("状态标签映射：valid / unknown / discovered", () => {
    assert.equal(status.validityLabel("valid"), "有效");
    assert.equal(status.validityTone("valid"), "success");
    assert.equal(status.validityLabel("unknown"), "未复核");
    assert.equal(status.validityLabel("discovered"), "未托管");
    assert.equal(status.validityTone("missingDirectory"), "danger");
  });

  await check("vendor / 公共目录 / 基准变动文案映射", () => {
    assert.equal(status.vendorLabel("notPhp"), "无需复制");
    assert.equal(status.vendorLabel("lockMismatch"), "lock 不一致");
    assert.equal(status.vendorTone("copyFailed"), "danger");
    assert.equal(status.sharedDirLabel("reused"), "已复用");
    assert.equal(status.sharedDirLabel("failed"), "失败");
    assert.equal(status.hasChangesLabel(null), "未复核");
    assert.equal(status.hasChangesLabel(true), "有变更");
  });

  await check("合并状态文案映射：contained / unmerged / targetMissing", () => {
    assert.equal(merge.mergeCellLabel("contained"), "已包含（疑似 squash）");
    assert.equal(merge.mergeCellLabel("unmerged"), "未合并");
    assert.equal(merge.mergeCellLabel("targetMissing"), "目标分支不存在");
    assert.equal(merge.mergeCellTone("unmerged"), "warning");
    assert.equal(merge.MERGE_NOT_CHECKED_LABEL, "未检查");
  });

  await check("备注预检：空串清除 / 去空白 / 51 字符拒绝 / 换行拒绝", () => {
    assert.deepEqual(note.validateNote("   "), { ok: true, value: null, message: null });
    assert.equal(note.validateNote("  等 QA  ").value, "等 QA");
    assert.equal(note.validateNote("a".repeat(51)).ok, false);
    assert.equal(note.validateNote("a\nb").ok, false);
    assert.equal(note.validateNote("a".repeat(50)).ok, true);
  });

  await check("基分支归一化预览：空串 / 裸名 / remote 前缀 / 未知 remote", () => {
    assert.equal(baseRef.previewNormalized("", ["origin"]).value, "origin/master");
    assert.equal(baseRef.previewNormalized("develop", ["origin"]).value, "origin/develop");
    assert.equal(baseRef.previewNormalized("develop", ["upstream"]).value, "upstream/develop");
    assert.equal(baseRef.previewNormalized("upstream/main", ["origin", "upstream"]).value, "upstream/main");
    assert.notEqual(baseRef.previewNormalized("nope/x", ["origin"]).error, null);
  });

  await check("localStorage 默认值（SSR / 无 window 时回退）", () => {
    assert.equal(storage.readHiddenZoneExpanded(), false);
    assert.equal(storage.readVisibleColumns(), null);
    assert.equal(storage.readExpandedIterations(), null);
  });

  await check("列定义：恒显列固定，未知 key 丢弃", () => {
    assert.deepEqual(columns.LOCKED_COLUMN_KEYS, ["project", "status", "actions"]);
    const resolved = columns.resolveVisibleColumns(["branch", "unknown-key"]);
    assert.ok(resolved.includes("project"));
    assert.ok(resolved.includes("status"));
    assert.ok(resolved.includes("actions"));
    assert.ok(resolved.includes("branch"));
    assert.ok(!resolved.includes("unknown-key"));
  });

  await check("AppSidebar 渲染「工作区」「设置」且当前项高亮", async () => {
    const html = await render("/src/components/AppSidebar.vue", { page: "workspaces" });
    assert.ok(html.includes("工作区"));
    assert.ok(html.includes("设置"));
    // Vue SSR 会把动态 class 排在静态 class 之前
    assert.ok(html.includes('class="active nav-item"'));
    assert.ok(html.includes('class="nav-item"'));
  });

  await check("WorkspaceList 渲染标题 / 复核按钮 / 迭代与项目 / 已隐藏收纳区", async () => {
    const html = await render("/src/components/WorkspaceList.vue", {
      groups: demo.demoWorkspaceGroups,
      reconcileLoading: false,
      emptyMessage: "请先在设置中填写工作区根目录",
      mergeResults: new Map(),
    });
    assert.ok(html.includes("工作区"));
    assert.ok(html.includes("复核状态"));
    assert.ok(html.includes("7.3.0"));
    assert.ok(html.includes("api3"));
    assert.ok(html.includes("已隐藏迭代（1）"));
    assert.ok(html.includes("清单不可用"));
    assert.ok(html.includes("未复核"));
    assert.ok(html.includes("已同步改名"));
  });

  await check("IterationCard 渲染头部按钮 / 表格 / 状态标签", async () => {
    const html = await render("/src/components/IterationCard.vue", {
      group: demo.demoWorkspaceGroups[0],
      visibleColumns: columns.COLUMNS.map((column) => column.key),
      expanded: true,
      filterActive: false,
      mergeResult: null,
    });
    assert.ok(html.includes("✎ 备注"));
    assert.ok(html.includes("归档"));
    assert.ok(html.includes("检查合并"));
    assert.ok(html.includes("基于 origin/master"));
    assert.ok(html.includes("未检查"));
    assert.ok(html.includes("Worktree 路径"));
    assert.ok(html.includes("目录缺失"));
  });

  await check("RemovalDialog 渲染风险列表 / 确认文本 / 阻止标记", async () => {
    const html = await render("/src/components/RemovalDialog.vue", {
      iteration: "7.3.0",
      project: demo.demoWorkspaceGroups[0].projects[0],
      assessment: demo.demoRemovalAssessment,
    });
    assert.ok(html.includes("7.3.0/api3"));
    assert.ok(html.includes("存在未跟踪文件"));
    assert.ok(html.includes("阻止"));
    assert.ok(html.includes("移除"));
  });

  await check("ArchiveDialog 渲染不干净态 / 强制勾选 / 确认输入", async () => {
    const html = await render("/src/components/ArchiveDialog.vue", {
      iteration: "7.3.0",
      assessment: demo.demoArchiveAssessment,
    });
    assert.ok(html.includes("不干净"));
    assert.ok(html.includes("强制归档"));
    assert.ok(html.includes("7.3.0"));
  });

  await check("NoteTooltip：关闭态只渲染标记 ▤，不渲染气泡内容", async () => {
    const html = await render("/src/components/NoteTooltip.vue", {
      note: "这是一段备注",
      id: "note-tip-7.3.0",
    });
    assert.ok(html.includes("▤"), "标记应始终渲染");
    assert.ok(html.includes('tabindex="0"'), "标记可聚焦（键盘用户可用）");
    assert.ok(!html.includes("这是一段备注"), "关闭态不渲染备注全文");
    assert.ok(!html.includes("aria-describedby"), "关闭态不设置 aria-describedby");
  });

  await check("OperationView 渲染阶段标签与结果汇总", async () => {
    const html = await render("/src/components/OperationView.vue", {
      items: [
        { projectId: "api3", index: 1, total: 2, phase: "fetching", message: "git fetch origin master" },
        { projectId: "gateway", index: 2, total: 2, phase: "failed", message: "git worktree add 失败" },
      ],
      running: false,
      result: {
        iteration: "7.3.0",
        iterationPath: "/Users/demo/Work/workspace/7.3.0",
        sharedDirectories: [],
        projects: [
          {
            projectId: "api3",
            worktreePath: "/Users/demo/Work/workspace/7.3.0/api3",
            status: "created",
            message: null,
            vendorStatus: "copied",
          },
          {
            projectId: "gateway",
            worktreePath: "/Users/demo/Work/workspace/7.3.0/gateway",
            status: "failed",
            message: "远端分支不存在",
            vendorStatus: null,
          },
        ],
        aborted: false,
        abortReason: null,
      },
      errorMessage: null,
    });
    assert.ok(html.includes("git fetch origin master"));
    assert.ok(html.includes("失败"));
    assert.ok(html.includes("已创建"));
  });
  await check("创建页联动：统一值覆盖已勾选行 / 新勾选行继承 / 单行不反向影响", async () => {
    const form = await server.ssrLoadModule("/src/utils/create-form.ts");
    const rows = form.buildRows([
      { id: "api3", repositoryPath: "/repo/api3", projectType: "php", vendorAvailable: true },
      { id: "gateway", repositoryPath: "/repo/gateway", projectType: "go", vendorAvailable: false },
    ]);
    assert.equal(rows.length, 2);
    assert.ok(rows.every((row) => !row.selected && row.branch === "" && row.baseRef === ""));

    // 新勾选行继承当前统一值
    form.toggleRowSelection(rows[0], "feature/7.3.0", "origin/release");
    assert.equal(rows[0].branch, "feature/7.3.0");
    assert.equal(rows[0].baseRef, "origin/release");
    assert.equal(rows[1].branch, "");

    // 修改统一值覆盖已勾选行，不影响未勾选行
    form.applyUnifiedBranch(rows, "feature/other");
    form.applyUnifiedBaseRef(rows, "origin/master");
    assert.equal(rows[0].branch, "feature/other");
    assert.equal(rows[0].baseRef, "origin/master");
    assert.equal(rows[1].branch, "");

    // 单行修改不反向影响统一值（统一值由调用方变量持有，rows 不回写）
    rows[0].branch = "feature/manual";
    assert.equal(rows[0].branch, "feature/manual");

    // 请求组装：空白分支 → null；未勾选行不进入请求
    const requests = form.toProjectRequests(rows);
    assert.equal(requests.length, 1);
    assert.deepEqual(requests[0], {
      projectId: "api3",
      branch: "feature/manual",
      baseRef: "origin/master",
    });
    form.toggleRowSelection(rows[1], "", "");
    const all = form.toProjectRequests(rows);
    assert.equal(all.length, 2);
    assert.equal(all[1].branch, null, "留空分支名＝Detached HEAD");
  });

  await check("localStorage 三键读写与损坏回退（注入假 window.localStorage）", async () => {
    const store = new Map();
    globalThis.window = {
      localStorage: {
        getItem: (key) => (store.has(key) ? store.get(key) : null),
        setItem: (key, value) => {
          store.set(key, value);
        },
        removeItem: (key) => {
          store.delete(key);
        },
      },
    };
    try {
      assert.equal(storage.readHiddenZoneExpanded(), false);
      assert.equal(storage.readVisibleColumns(), null);

      storage.writeVisibleColumns(["project", "status"]);
      assert.deepEqual(storage.readVisibleColumns(), ["project", "status"]);
      storage.writeExpandedIterations(["7.3.0"]);
      assert.deepEqual(storage.readExpandedIterations(), ["7.3.0"]);
      storage.writeHiddenZoneExpanded(true);
      assert.equal(storage.readHiddenZoneExpanded(), true);

      // 损坏 JSON → 回退默认值并覆盖写入
      store.set("worktree-manager.visible-columns", "{不是 JSON");
      assert.equal(storage.readVisibleColumns(), null);
      assert.equal(store.get("worktree-manager.visible-columns"), "[]");
    } finally {
      delete globalThis.window;
    }
  });

  await check("SettingsView 空配置渲染「必填」标记与空表", async () => {
    const html = await render("/src/components/SettingsView.vue", {
      config: {
        schemaVersion: 1,
        workspaceRoot: null,
        sharedDirectories: [],
        projects: [],
        recentIterations: [],
      },
    });
    assert.ok(html.includes("工作区根目录"));
    assert.ok(html.includes("required-mark"));
    assert.ok(html.includes("还没有公共目录规则"));
    assert.ok(html.includes("还没有项目"));
    assert.ok(html.includes("选择目录"));
  });

  await check("CreateWorkspace 在 workspaceRoot 为空时禁用「开始创建」并给出原因", async () => {
    const html = await render("/src/components/CreateWorkspace.vue", {
      config: {
        schemaVersion: 1,
        workspaceRoot: null,
        sharedDirectories: [],
        projects: [
          { id: "api3", repositoryPath: "/repo/api3", projectType: "php", vendorAvailable: true },
        ],
        recentIterations: ["7.3.0"],
      },
    });
    assert.ok(html.includes("请先在设置中填写工作区根目录"));
    assert.ok(html.includes("开始创建"));
    assert.ok(html.includes("disabled"));
    assert.ok(html.includes("7.3.0"));
    assert.ok(html.includes("api3"));
  });
  await check("合并检查事件应用：新增 / 就地替换 / 非 record 阶段不改动", async () => {
    const merge = await server.ssrLoadModule("/src/utils/merge.ts");
    const baseRecord = {
      projectId: "api3",
      branchDisplay: "feature/x",
      worktreePath: "/ws/7.3.0/api3",
      lifecycle: "active",
      develop: { status: "merged", unmergedCommits: [], errorMessage: null, stale: false, dirty: false },
      master: { status: "unmerged", unmergedCommits: ["9f8e7d6 修正"], errorMessage: null, stale: false, dirty: false },
      hasChanges: false,
      dirty: false,
    };
    let results = new Map();
    const unchanged = merge.applyRecord(results, {
      iteration: "7.3.0",
      projectId: "api3",
      worktreePath: "/ws/7.3.0/api3",
      index: 0,
      total: 1,
      phase: "checking",
      message: "git merge-base --is-ancestor feature/x origin/develop",
      record: null,
    });
    assert.equal(unchanged.size, 0, "非 record 阶段不改动结果");

    results = merge.applyRecord(results, {
      iteration: "7.3.0",
      projectId: "api3",
      worktreePath: "/ws/7.3.0/api3",
      index: 1,
      total: 1,
      phase: "record",
      message: "完成",
      record: baseRecord,
    });
    assert.equal(results.get("7.3.0").records.length, 1);

    // 同 worktreePath 就地替换
    const updated = { ...baseRecord, develop: { ...baseRecord.develop, status: "contained" } };
    results = merge.applyRecord(results, {
      iteration: "7.3.0",
      projectId: "api3",
      worktreePath: "/ws/7.3.0/api3",
      index: 1,
      total: 1,
      phase: "record",
      message: "完成",
      record: updated,
    });
    assert.equal(results.get("7.3.0").records.length, 1, "不应重复追加");
    assert.equal(results.get("7.3.0").records[0].develop.status, "contained");
  });

  await check("copyText：无 navigator / document 时 reject（不抛同步异常）", async () => {
    const clipboard = await server.ssrLoadModule("/src/utils/clipboard.ts");
    const savedNavigator = globalThis.navigator;
    const savedDocument = globalThis.document;
    try {
      Object.defineProperty(globalThis, "navigator", { value: undefined, configurable: true });
      globalThis.document = undefined;
      await assert.rejects(async () => clipboard.copyText("x"), /复制失败/);
    } finally {
      if (savedNavigator === undefined) {
        delete globalThis.navigator;
      } else {
        Object.defineProperty(globalThis, "navigator", { value: savedNavigator, configurable: true });
      }
      if (savedDocument === undefined) {
        delete globalThis.document;
      } else {
        globalThis.document = savedDocument;
      }
    }
  });

  await check("copyText：优先 navigator.clipboard（不创建 textarea）", async () => {
    const clipboard = await server.ssrLoadModule("/src/utils/clipboard.ts");
    const savedNavigator = globalThis.navigator;
    let written = null;
    let textareaCreated = false;
    try {
      Object.defineProperty(globalThis, "navigator", {
        value: { clipboard: { writeText: async (text) => {
          written = text;
        } } },
        configurable: true,
      });
      globalThis.document = { createElement: () => {
        textareaCreated = true;
        return { style: {}, setAttribute: () => {}, select: () => {} };
      }, body: { appendChild: () => {}, removeChild: () => {} }, execCommand: () => true };
      await clipboard.copyText("完整路径 /ws/7.3.0/api3");
    } finally {
      delete globalThis.document;
      if (savedNavigator === undefined) {
        delete globalThis.navigator;
      } else {
        Object.defineProperty(globalThis, "navigator", { value: savedNavigator, configurable: true });
      }
    }
    assert.equal(written, "完整路径 /ws/7.3.0/api3");
    assert.equal(textareaCreated, false, "navigator.clipboard 可用时不应创建 textarea");
  });

  await check("IterationCard 两个可复制格含 copyable，且渲染「已复制」占位逻辑", async () => {
    const form = await server.ssrLoadModule("/src/utils/columns.ts");
    const demoGroup = demo.demoWorkspaceGroups[0];
    const html = await render("/src/components/IterationCard.vue", {
      group: demoGroup,
      visibleColumns: form.COLUMNS.map((column) => column.key),
      expanded: true,
      filterActive: false,
      mergeResult: null,
      checking: false,
      progressLine: "git fetch --no-tags origin develop master",
      dragging: false,
    });
    assert.ok(html.includes("copyable"), "路径格与分支格应有 copyable 类");
    assert.ok(html.includes("git fetch --no-tags origin develop master"), "fetching 阶段显示命令");
    assert.ok(html.includes("未检查"));
  });

  await check("归档按钮可用性纯函数 canArchive", async () => {
    const archiveUtils = await server.ssrLoadModule("/src/utils/archive.ts");
    assert.equal(archiveUtils.canArchive(true, "7.3.0", "7.3.0", false), true, "干净 + 确认文本正确");
    assert.equal(archiveUtils.canArchive(true, "7.3.0 ", "7.3.0", false), false, "确认文本必须严格相等");
    assert.equal(archiveUtils.canArchive(false, "7.3.0", "7.3.0", false), false, "不干净且未勾选强制");
    assert.equal(archiveUtils.canArchive(false, "7.3.0", "7.3.0", true), true, "不干净但已勾选强制");
  });

  await check("ArchiveDialog 干净态不渲染强制勾选，不干净态渲染 blockers 与强制勾选", async () => {
    const cleanHtml = await render("/src/components/ArchiveDialog.vue", {
      iteration: "7.3.0",
      assessment: {
        iteration: "7.3.0",
        confirmationText: "7.3.0",
        checkedAt: "2026-07-31T09:00:00Z",
        clean: true,
        records: [],
      },
      progressLine: null,
    });
    assert.ok(cleanHtml.includes("归档"));
    assert.ok(!cleanHtml.includes("强制归档"), "干净态不应出现强制归档");

    const dirtyHtml = await render("/src/components/ArchiveDialog.vue", {
      iteration: "7.3.0",
      assessment: demo.demoArchiveAssessment,
      progressLine: "git worktree remove /ws/7.3.0/api3",
    });
    assert.ok(dirtyHtml.includes("强制归档"));
    assert.ok(dirtyHtml.includes("master：未合并 1 个提交"), "blockers 文案应显示");
    assert.ok(dirtyHtml.includes("git worktree remove /ws/7.3.0/api3"), "执行阶段显示实际命令");
  });

  await check("IterationCard：无备注不渲染 ▤，有备注渲染 ▤；归档评估中按钮显示「评估中…」", async () => {
    const columnsUtil = await server.ssrLoadModule("/src/utils/columns.ts");
    const columns = columnsUtil.COLUMNS.map((column) => column.key);
    const group = demo.demoWorkspaceGroups[0];

    const withoutNote = await render("/src/components/IterationCard.vue", {
      group: { ...group, note: null },
      visibleColumns: columns,
      expanded: true,
      filterActive: false,
      mergeResult: null,
      checking: false,
      progressLine: null,
      dragging: false,
      archiving: false,
    });
    assert.ok(!withoutNote.includes("▤"), "无备注不应渲染标记");

    const withNote = await render("/src/components/IterationCard.vue", {
      group: { ...group, note: "等待回归" },
      visibleColumns: columns,
      expanded: true,
      filterActive: false,
      mergeResult: null,
      checking: false,
      progressLine: null,
      dragging: false,
      archiving: true,
    });
    assert.ok(withNote.includes("▤"));
    assert.ok(withNote.includes("评估中…"));
  });


  await check("WorkspaceList：0 个隐藏迭代时不渲染收纳区，2 个时渲染计数", async () => {
    const visibleGroup = { ...demo.demoWorkspaceGroups[0], hiddenAt: null };
    const withoutHidden = await render("/src/components/WorkspaceList.vue", {
      groups: [visibleGroup],
      reconcileLoading: false,
      emptyMessage: "无",
      archiveAssessing: null,
    });
    assert.ok(!withoutHidden.includes("已隐藏迭代"), "无隐藏项时整区不渲染");

    const hiddenOne = { ...demo.demoWorkspaceGroups[0], iteration: "7.2.0", hiddenAt: "2026-07-20T08:00:00Z" };
    const hiddenTwo = { ...demo.demoWorkspaceGroups[0], iteration: "7.2.1", hiddenAt: "2026-07-21T08:00:00Z" };
    const withHidden = await render("/src/components/WorkspaceList.vue", {
      groups: [visibleGroup, hiddenOne, hiddenTwo],
      reconcileLoading: false,
      emptyMessage: "无",
      archiveAssessing: null,
    });
    assert.ok(withHidden.includes("已隐藏迭代（2）"));
    assert.ok(!withHidden.includes("已隐藏迭代（2）") === false);
  });


  await check("IterationCard 渲染「已同步改名」标签；countRenamed 统计正确", async () => {
    const statusUtil = await server.ssrLoadModule("/src/utils/status.ts");
    const columnsUtil = await server.ssrLoadModule("/src/utils/columns.ts");
    const columns = columnsUtil.COLUMNS.map((column) => column.key);
    const group = demo.demoWorkspaceGroups[0];
    const props = {
      visibleColumns: columns,
      expanded: true,
      filterActive: false,
      mergeResult: null,
      checking: false,
      progressLine: null,
      dragging: false,
      archiving: false,
    };

    const renamedHtml = await render("/src/components/IterationCard.vue", { ...props, group });
    assert.ok(renamedHtml.includes("已同步改名"), "renamedFrom 非空应显示标签");
    assert.ok(renamedHtml.includes("原分支") || renamedHtml.includes("原名"), "title 应含旧名");

    const plain = {
      ...group,
      projects: group.projects.map((project) => ({ ...project, renamedFrom: null })),
    };
    const plainHtml = await render("/src/components/IterationCard.vue", { ...props, group: plain });
    assert.ok(!plainHtml.includes("已同步改名"), "renamedFrom 为空不应显示标签");

    assert.equal(statusUtil.countRenamed([group]), 1);
    assert.equal(statusUtil.countRenamed([plain]), 0);
    assert.equal(statusUtil.countRenamed([group, plain]), 1);
    assert.equal(statusUtil.countRenamed([]), 0);
  });


  await check("CreateWorkspace：基分支候选 datalist、⟳ 按钮与预览文案", async () => {
    const html = await render("/src/components/CreateWorkspace.vue", {
      config: {
        schemaVersion: 1,
        workspaceRoot: "/Users/me/Work/workspace",
        sharedDirectories: [],
        projects: [
          { id: "api3", repositoryPath: "/repo/api3", projectType: "php", vendorAvailable: true },
        ],
        recentIterations: ["7.3.0"],
      },
    });
    assert.ok(html.includes('id="unified-base-ref"'), "统一基分支应有 datalist");
    assert.ok(html.includes('id="base-ref-api3"'), "逐项目应有 datalist");
    assert.ok(html.includes("⟳"), "应有刷新按钮");
    assert.ok(html.includes("将解析为：origin/master"), "空输入应预览为 origin/master");
  });

  await check("IterationCard：仅 active 行渲染可拖手柄，落点显示指示线", async () => {
    const columnsUtil = await server.ssrLoadModule("/src/utils/columns.ts");
    const columns = columnsUtil.COLUMNS.map((column) => column.key);
    const group = demo.demoWorkspaceGroups[0];
    const baseProps = {
      visibleColumns: columns,
      expanded: true,
      filterActive: false,
      mergeResult: null,
      checking: false,
      progressLine: null,
      dragging: false,
      archiving: false,
      dragSourcePath: null,
      dropArmed: false,
      dropBeforePath: null,
    };
    const html = await render("/src/components/IterationCard.vue", { ...baseProps, group });
    const handles = html.split("drag-handle").length - 1;
    const draggables = html.split('draggable="true"').length - 1;
    assert.ok(handles >= 1, "应有拖拽手柄");
    // 演示数据里 4 条记录中只有 3 条 active（createFailed 不渲染手柄）
    assert.equal(draggables, 3, `可拖手柄数应为 3，实际 ${draggables}`);
    assert.ok(!html.includes("drop-indicator"), "未拖拽时不应有落点指示");

    const arming = await render("/src/components/IterationCard.vue", {
      ...baseProps,
      group,
      dropArmed: true,
      dropBeforePath: group.projects[1].worktreePath,
      dragSourcePath: group.projects[0].worktreePath,
    });
    assert.ok(arming.includes("drop-indicator"), "落点行应显示指示线");
    assert.ok(arming.includes("row-dragging"), "拖拽中的行应加视觉标记");
  });

  await check("基分支预览：未知 remote 给出错误文案", async () => {
    const baseRef = await server.ssrLoadModule("/src/utils/base-ref.ts");
    const preview = baseRef.previewNormalized("nope/x", ["origin"]);
    assert.notEqual(preview.error, null);
    assert.ok(preview.error.includes("remote"));
    assert.equal(baseRef.previewNormalized("release", ["origin"]).value, "origin/release");
  });

} catch (error) {
  failed += 1;
  console.error("测试执行中断：", error);
} finally {
  await server.close();
}

console.log(`\n前端 SSR 测试：通过 ${passed} 项，失败 ${failed} 项`);
process.exit(failed === 0 ? 0 : 1);


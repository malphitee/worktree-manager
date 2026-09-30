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
    assert.equal(baseRef.normalizePreview("", ["origin"]).value, "origin/master");
    assert.equal(baseRef.normalizePreview("develop", ["origin"]).value, "origin/develop");
    assert.equal(baseRef.normalizePreview("develop", ["upstream"]).value, "upstream/develop");
    assert.equal(baseRef.normalizePreview("upstream/main", ["origin", "upstream"]).value, "upstream/main");
    assert.notEqual(baseRef.normalizePreview("nope/x", ["origin"]).error, null);
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

  await check("NoteTooltip 关闭态不渲染内容", async () => {
    const html = await render("/src/components/NoteTooltip.vue", {
      text: "这是一段备注",
      anchor: null,
    });
    assert.ok(!html.includes("这是一段备注"));
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
} catch (error) {
  failed += 1;
  console.error("测试执行中断：", error);
} finally {
  await server.close();
}

console.log(`\n前端 SSR 测试：通过 ${passed} 项，失败 ${failed} 项`);
process.exit(failed === 0 ? 0 : 1);


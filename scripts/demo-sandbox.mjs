#!/usr/bin/env node
// 本地演示沙箱（开发工具，不参与构建、不引入任何依赖）：
// 在 ~/wm-demo 下生成 3 个带 master/develop/已合并与未合并分支的仓库、一个公共目录源与工作区根目录，
// 并可选地把沙箱配置写入应用配置目录（写入前自动备份已有配置）。
//
// 用法：
//   node scripts/demo-sandbox.mjs                 # 只建沙箱，打印后续手动步骤
//   node scripts/demo-sandbox.mjs --write-config  # 同时备份并写入沙箱配置
//   node scripts/demo-sandbox.mjs --dir /tmp/x    # 换目录（测试用）
//   node scripts/demo-sandbox.mjs --force         # 目录已存在时先删除重建
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const demoDir = args.includes("--dir") ? args[args.indexOf("--dir") + 1] : path.join(homedir(), "wm-demo");
const writeConfig = args.includes("--write-config");
const force = args.includes("--force");

if (existsSync(demoDir) && !force) {
  console.error(`目录已存在：${demoDir}`);
  console.error("如需重建请加 --force（会删除该目录内的全部内容）");
  process.exit(1);
}
if (existsSync(demoDir)) {
  rmSync(demoDir, { recursive: true, force: true });
}

const repos = path.join(demoDir, "repos");
const remotes = path.join(demoDir, "remotes");
const workspace = path.join(demoDir, "workspace");
const common = path.join(demoDir, "fd-common");
for (const dir of [repos, remotes, workspace, common]) {
  mkdirSync(dir, { recursive: true });
}
writeFileSync(path.join(common, "index.php"), "<?php\n// 公共目录快照示例\n", "utf8");
writeFileSync(path.join(common, "helper.js"), "// shared helper\n", "utf8");

const gitEnv = {
  ...process.env,
  GIT_AUTHOR_NAME: "WM Demo",
  GIT_AUTHOR_EMAIL: "demo@example.com",
  GIT_COMMITTER_NAME: "WM Demo",
  GIT_COMMITTER_EMAIL: "demo@example.com",
  GIT_TERMINAL_PROMPT: "0",
};

function git(cwd, ...gitArgs) {
  return execFileSync("git", gitArgs, { cwd, env: gitEnv, stdio: "pipe" }).toString();
}

/** 每个项目：master / develop / feature-merged（已合入 develop）/ feature-pending（未合并） */
function buildProject(name, kind) {
  const remote = path.join(remotes, `${name}.git`);
  const seed = path.join(repos, name);
  mkdirSync(seed, { recursive: true });
  git(remotes, "init", "--bare", "--initial-branch=master", remote);
  git(seed, "init", "--initial-branch=master");
  writeFileSync(path.join(seed, "README.md"), `# ${name}\n`, "utf8");
  // 只提交明确列出的文件：PHP 项目的 vendor 必须保持「未跟踪且未被忽略」
  const tracked = ["README.md"];
  if (kind === "php") {
    writeFileSync(path.join(seed, "composer.json"), "{}\n", "utf8");
    writeFileSync(path.join(seed, "composer.lock"), '{"packages": []}\n', "utf8");
    mkdirSync(path.join(seed, "vendor", "acme"), { recursive: true });
    writeFileSync(path.join(seed, "vendor", "autoload.php"), "<?php\n", "utf8");
    tracked.push("composer.json", "composer.lock");
  }
  if (kind === "go") {
    writeFileSync(path.join(seed, "go.mod"), `module demo/${name}\n`, "utf8");
    tracked.push("go.mod");
  }
  git(seed, "add", ...tracked);
  git(seed, "commit", "-m", "初始提交");
  git(seed, "remote", "add", "origin", remote);
  git(seed, "push", "-q", "origin", "master");
  git(seed, "checkout", "-q", "-b", "develop");
  git(seed, "push", "-q", "origin", "develop");
  git(seed, "checkout", "-q", "master");
  for (const [branch, file, message] of [
    ["feature/merged", "merged.txt", "已完成并合并的改动"],
    ["feature/pending", "pending.txt", "尚未合并的改动"],
  ]) {
    git(seed, "checkout", "-q", "-b", branch);
    writeFileSync(path.join(seed, file), `${message}\n`, "utf8");
    git(seed, "add", file);
    git(seed, "commit", "-m", message);
    git(seed, "push", "-q", "origin", branch);
    git(seed, "checkout", "-q", "master");
  }
  git(seed, "checkout", "-q", "develop");
  git(seed, "merge", "-q", "--no-ff", "-m", "合并 feature/merged", "origin/feature/merged");
  git(seed, "push", "-q", "origin", "develop");
  git(seed, "checkout", "-q", "master");
}

buildProject("api3", "php");
buildProject("web-portal", "other");
buildProject("gateway", "go");

const config = {
  schemaVersion: 1,
  workspaceRoot: workspace,
  sharedDirectories: [{ sourcePath: common, targetDirectory: "fd-common" }],
  projects: [
    { id: "api3", repositoryPath: path.join(repos, "api3"), projectType: "php", vendorAvailable: true },
    { id: "web-portal", repositoryPath: path.join(repos, "web-portal"), projectType: "other", vendorAvailable: false },
    { id: "gateway", repositoryPath: path.join(repos, "gateway"), projectType: "go", vendorAvailable: false },
  ],
  recentIterations: ["7.3.0", "7.2.0"],
};

console.log(`沙箱已就绪：${demoDir}`);
console.log(`  仓库：${repos}/{api3,web-portal,gateway}（各含 master / develop / feature/merged / feature/pending）`);
console.log(`  公共目录源：${common}；工作区根目录：${workspace}`);

if (!writeConfig) {
  console.log("\n未写入应用配置。要启用沙箱，请加 --write-config 运行（会先备份现有配置）。");
  process.exit(0);
}

const configDir = path.join(homedir(), "Library", "Application Support", "WorktreeManager");
mkdirSync(configDir, { recursive: true });
const configPath = path.join(configDir, "config.json");
if (existsSync(configPath)) {
  const backup = `${configPath}.bak-${Date.now()}`;
  writeFileSync(backup, readFileSync(configPath));
  console.log(`已备份现有配置：${backup}`);
}
writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");
console.log(`已写入沙箱配置：${configPath}`);

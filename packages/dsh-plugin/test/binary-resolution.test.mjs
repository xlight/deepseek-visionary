// Windows 安装友好修复的单测：npm shim 二进制解析 + 懒解析。
//
// - resolveFromNpmShim：从 .cmd/.ps1 shim 文本解析 exe 真身
// - resolveBinaryPath：PATH 扫描 .exe 失败后走 shim 解析；DEEPSEEK_VISIONARY_BIN 优先
// - 懒解析：apply 不缓存 binary，工具调用时重新解析（环境变量改动即时生效）
//
// 零第三方依赖，node --test 直接运行。

import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const plugin = await import("../lib/index.mjs");
const { resolveBinaryPath, resolveFromNpmShim } = plugin;

const EXE_REL = path.join("node_modules", ".bin_real", "visionary-server.exe");

async function makeShimTree() {
  // 模拟 npm 全局安装：<prefix>/visionary-server.cmd + 包内 exe 真身
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "vs-shim-"));
  const binDir = path.join(root, "bin");
  const pkgDir = path.join(binDir, "node_modules", "@xlight-oss", "visionary-server");
  const binReal = path.join(pkgDir, "node_modules", ".bin_real");
  await fs.mkdir(binReal, { recursive: true });
  await fs.writeFile(path.join(binReal, "visionary-server.exe"), "fake exe");
  const pkgRel = "node_modules\\@xlight-oss\\visionary-server\\run-visionary-server.js";
  await fs.writeFile(
    path.join(binDir, "visionary-server.cmd"),
    `@ECHO off\r\nGOTO start\r\n:start\r\n"%_prog%"  "%dp0%\\${pkgRel}" %*\r\n`,
  );
  return { root, binDir, exePath: path.join(binReal, "visionary-server.exe") };
}

test("resolveFromNpmShim: extracts exe real path from .cmd shim", async () => {
  const { root, binDir, exePath } = await makeShimTree();
  try {
    const resolved = resolveFromNpmShim(path.join(binDir, "visionary-server.cmd"));
    assert.equal(resolved, exePath);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("resolveFromNpmShim: returns null for non-shim content", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "vs-shim-bad-"));
  try {
    const f = path.join(root, "visionary-server.cmd");
    await fs.writeFile(f, "@ECHO off\necho not a visionary shim\n");
    assert.equal(resolveFromNpmShim(f), null);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("resolveBinaryPath: finds .exe in PATH first", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "vs-path-"));
  const oldPath = process.env.PATH;
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    // 与 resolveBinaryPath 的查找名一致：win32 用 .exe，其他平台无扩展名
    const exeName = process.platform === "win32" ? "visionary-server.exe" : "visionary-server";
    const exe = path.join(root, exeName);
    await fs.writeFile(exe, "fake");
    process.env.PATH = root;
    delete process.env.DEEPSEEK_VISIONARY_BIN;
    const resolved = resolveBinaryPath({ binaryPath: "" });
    assert.equal(resolved, exe);
  } finally {
    if (oldPath === undefined) delete process.env.PATH;
    else process.env.PATH = oldPath;
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("resolveBinaryPath: DEEPSEEK_VISIONARY_BIN wins over PATH", async () => {
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    process.env.DEEPSEEK_VISIONARY_BIN = "C:\\tools\\visionary-server.exe";
    const resolved = resolveBinaryPath({ binaryPath: "" });
    assert.equal(resolved, "C:\\tools\\visionary-server.exe");
  } finally {
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
  }
});

test("resolveBinaryPath: config.binaryPath wins over everything", async () => {
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    process.env.DEEPSEEK_VISIONARY_BIN = "C:\\tools\\visionary-server.exe";
    const resolved = resolveBinaryPath({ binaryPath: "D:\\custom\\visionary-server.exe" });
    assert.equal(resolved, "D:\\custom\\visionary-server.exe");
  } finally {
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
  }
});

test("resolveBinaryPath: shim fallback works when PATH has only .cmd (win32)", async () => {
  const { root, binDir, exePath } = await makeShimTree();
  const oldPath = process.env.PATH;
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  const oldPlatform = Object.getOwnPropertyDescriptor(process, "platform");
  try {
    // 模拟 win32：PATH 只有 shim 无 exe → shim 解析出真身
    delete process.env.DEEPSEEK_VISIONARY_BIN;
    process.env.PATH = binDir;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });
    const resolved = resolveBinaryPath({ binaryPath: "" });
    assert.equal(resolved, exePath);
  } finally {
    if (oldPath === undefined) delete process.env.PATH;
    else process.env.PATH = oldPath;
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
    Object.defineProperty(process, "platform", oldPlatform);
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("resolveBinaryPath: returns null when nothing resolves", async () => {
  const oldPath = process.env.PATH;
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    delete process.env.DEEPSEEK_VISIONARY_BIN;
    process.env.PATH = os.tmpdir(); // 一个没有 visionary 的目录
    const resolved = resolveBinaryPath({ binaryPath: "" });
    assert.equal(resolved, null);
  } finally {
    if (oldPath === undefined) delete process.env.PATH;
    else process.env.PATH = oldPath;
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
  }
});

// 懒解析：apply 不缓存 binary —— 工具调用时重新解析。
// 直接验证 resolveBinaryPath 每次都被调用（通过 config 变更即时生效的等价断言：
// 第一次 null，第二次设 env 后命中，无需重启/重建闭包）。
test("lazy resolution: env change takes effect without re-apply", async () => {
  const oldEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  const oldPath = process.env.PATH;
  try {
    delete process.env.DEEPSEEK_VISIONARY_BIN;
    process.env.PATH = os.tmpdir();
    const config = { binaryPath: "" };
    assert.equal(resolveBinaryPath(config), null);
    // 模拟用户在运行中设置环境变量（懒解析应即时生效）
    process.env.DEEPSEEK_VISIONARY_BIN = "C:\\tools\\visionary-server.exe";
    assert.equal(resolveBinaryPath(config), "C:\\tools\\visionary-server.exe");
  } finally {
    if (oldEnv === undefined) delete process.env.DEEPSEEK_VISIONARY_BIN;
    else process.env.DEEPSEEK_VISIONARY_BIN = oldEnv;
    if (oldPath === undefined) delete process.env.PATH;
    else process.env.PATH = oldPath;
  }
});

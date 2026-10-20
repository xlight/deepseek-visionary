// Unit tests for lib/dashboard.mjs — VisionaryDashboard host Service.
//
// Uses a real cordis Context (because TypertRemoteService's constructor calls
// ctx.reflect.provide and ctx.root) with stubbed sibling entry configs and a
// fake visionary-server binary written into a tmpdir.

import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let cordis;
try {
  cordis = require("@deepseek-ai/cordis");
} catch {
  cordis = null;
}
const skip = cordis === null
  ? "node_modules not installed — run `pnpm install` in packages/dsh-plugin to enable"
  : false;

// Windows spawn() cannot exec shebang scripts or .cmd files without shell:true,
// and runCli does not set shell:true. Binary-spawning tests run only on POSIX.
const skipBin = process.platform === "win32"
  ? "binary spawn tests skipped on Windows (runCli does not set shell:true)"
  : skip;

const tmpRoots = [];
async function tmpDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "vdash-test-"));
  tmpRoots.push(dir);
  return dir;
}
test.after(async () => {
  await Promise.all(tmpRoots.map((d) => fs.rm(d, { recursive: true, force: true })));
});

async function writeFakeBin(dir, versionLine, statusJson, loginExitCode) {
  const isWin = process.platform === "win32";
  const scriptName = isWin ? "fake-visionary.js" : "fake-visionary";
  const scriptPath = path.join(dir, scriptName);
  const script = `#!/usr/bin/env node
const args = process.argv.slice(2);
if (args[0] === "--version") { process.stdout.write(${JSON.stringify(versionLine)}); process.exit(0); }
if (args[0] === "status" && args[1] === "--json") { process.stdout.write(${JSON.stringify(statusJson)}); process.exit(0); }
if (args[0] === "login") { process.exit(${loginExitCode ?? 0}); }
process.exit(2);
`;
  await fs.writeFile(scriptPath, script, { mode: 0o755 });
  if (isWin) {
    // Windows: spawn can't exec shebang; write a .cmd wrapper.
    const cmdPath = path.join(dir, "fake-visionary.cmd");
    await fs.writeFile(cmdPath, `@node "%~dp0fake-visionary.js" %*\r\n`);
    return cmdPath;
  }
  return scriptPath;
}

const { default: VisionaryDashboard } = await import("../lib/dashboard.mjs");

// 0.2.0 volatile semantics: fields arrive as live refs. volGet unwraps them.
// For tests we pass plain values; volGet returns non-Volatile values as-is.
const VOLATILE_WRITE = Symbol.for("cosmokit.volatile.write");
function volatileRef(initial) {
  let current = initial;
  return Object.freeze({ get: () => current, [VOLATILE_WRITE]: (next) => { current = next; } });
}
function volatileConfig(values) {
  const refs = {};
  for (const [key, value] of Object.entries(values)) refs[key] = volatileRef(value);
  return refs;
}

function makeCtx({ visionConfig, bridgeConfig } = {}) {
  const ctx = new cordis.Context();
  // Stub ctx.get so the dashboard can read sibling entry configs.
  const entries = new Map();
  if (visionConfig) entries.set("visionary-vision", { config: volatileConfig(visionConfig) });
  if (bridgeConfig) entries.set("visionary-image-bridge", { config: volatileConfig(bridgeConfig) });
  ctx.get = (id) => entries.get(id);
  return ctx;
}

test("getStatus returns binary=missing when binaryPath resolves to null", { skip: skipBin }, async () => {
  // Clear env so resolveBinaryPath falls through to PATH (which won't have
  // visionary-server in the test environment).
  const savedEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  delete process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    const ctx = makeCtx({
      visionConfig: { binaryPath: "", statusTimeoutMs: 1000 },
      bridgeConfig: { enabled: false },
    });
    const svc = new VisionaryDashboard(ctx, {
      visionEntryId: "visionary-vision",
      bridgeEntryId: "visionary-image-bridge",
    });
    const status = await svc.getStatus();
    assert.equal(status.binary.ready, false);
    assert.equal(status.binary.path, null);
    assert.equal(status.bridge.enabled, false);
    assert.equal(status.login.ok, false);
  } finally {
    if (savedEnv !== undefined) process.env.DEEPSEEK_VISIONARY_BIN = savedEnv;
  }
});

test("getStatus returns binary=ready + login=ok when fake binary responds", { skip: skipBin }, async () => {
  const binDir = await tmpDir();
  const pastedDir = await tmpDir();
  const bin = await writeFakeBin(binDir, "visionary-server 0.7.3\n", JSON.stringify({ ok: true, session: "abc" }), 0);
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: true, scope: "text-only", mode: "agentic", pastedDir },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const status = await svc.getStatus();
  assert.equal(status.binary.ready, true);
  assert.equal(status.binary.path, bin);
  assert.match(status.binary.version, /0\.7\.3/);
  assert.equal(status.login.ok, true);
  assert.equal(status.login.session, "abc");
  assert.equal(status.bridge.enabled, true);
  assert.equal(status.bridge.scope, "text-only");
  assert.equal(status.bridge.mode, "agentic");
  assert.equal(status.bridge.pastedCount, 0);
});

test("getStatus degrades gracefully when status command exits non-zero", { skip: skipBin }, async () => {
  const binDir = await tmpDir();
  const pastedDir = await tmpDir();
  const isWin = process.platform === "win32";
  const scriptPath = path.join(binDir, "fake-visionary-bad.js");
  await fs.writeFile(
    scriptPath,
    `const args = process.argv.slice(2);
if (args[0] === "--version") { process.stdout.write("visionary-server 0.7.3"); process.exit(0); }
if (args[0] === "status") { process.stderr.write("not logged in"); process.exit(1); }
process.exit(2);
`,
  );
  let bin = scriptPath;
  if (isWin) {
    bin = path.join(binDir, "fake-visionary-bad.cmd");
    await fs.writeFile(bin, `@node "%~dp0fake-visionary-bad.js" %*\r\n`);
  }
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: false },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const status = await svc.getStatus();
  assert.equal(status.binary.ready, true);
  assert.equal(status.login.ok, false);
  assert.match(status.login.error, /not logged in/);
});

test("reprobeBinary returns fresh binary status", { skip: skipBin }, async () => {
  const dir = await tmpDir();
  const bin = await writeFakeBin(dir, "visionary-server 0.7.3\n", "{}", 0);
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: false },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const result = await svc.reprobeBinary();
  assert.equal(result.ready, true);
  assert.equal(result.path, bin);
});

test("relogin throws RemoteError when binary missing", { skip: skipBin }, async () => {
  const savedEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  delete process.env.DEEPSEEK_VISIONARY_BIN;
  try {
    const ctx = makeCtx({
      visionConfig: { binaryPath: "", statusTimeoutMs: 1000 },
      bridgeConfig: { enabled: false },
    });
    const svc = new VisionaryDashboard(ctx, {
      visionEntryId: "visionary-vision",
      bridgeEntryId: "visionary-image-bridge",
    });
    await assert.rejects(() => svc.relogin(), (err) => {
      assert.match(err.message || String(err), /binary|visionary/i);
      return true;
    });
  } finally {
    if (savedEnv !== undefined) process.env.DEEPSEEK_VISIONARY_BIN = savedEnv;
  }
});

test("relogin succeeds when fake binary login exits 0", { skip: skipBin }, async () => {
  const dir = await tmpDir();
  const bin = await writeFakeBin(dir, "visionary-server 0.7.3\n", "{}", 0);
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: false },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const result = await svc.relogin();
  assert.equal(result.ok, true);
});

test("cleanPasted returns removed=0 when pastedDir empty", { skip }, async () => {
  const dir = await tmpDir();
  const ctx = makeCtx({
    visionConfig: { binaryPath: "", statusTimeoutMs: 1000 },
    bridgeConfig: { enabled: true, pastedDir: dir },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const result = await svc.cleanPasted();
  assert.equal(result.removed, 0);
});

test("cleanPasted removes files and returns count", { skip }, async () => {
  const dir = await tmpDir();
  await fs.writeFile(path.join(dir, "a.png"), "x");
  await fs.writeFile(path.join(dir, "b.png"), "y");
  const ctx = makeCtx({
    visionConfig: { binaryPath: "", statusTimeoutMs: 1000 },
    bridgeConfig: { enabled: true, pastedDir: dir },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const result = await svc.cleanPasted();
  assert.equal(result.removed, 2);
  const remaining = await fs.readdir(dir);
  assert.equal(remaining.length, 0);
});

test("selfCheck returns binary + login combined", { skip: skipBin }, async () => {
  const dir = await tmpDir();
  const bin = await writeFakeBin(dir, "visionary-server 0.7.3\n", JSON.stringify({ ok: true }), 0);
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: false },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const result = await svc.selfCheck();
  assert.equal(result.binary.ready, true);
  assert.equal(result.login.ok, true);
});

test("getStatus bridge.pastedCount counts files in pastedDir", { skip: skipBin }, async () => {
  const binDir = await tmpDir();
  const pastedDir = await tmpDir();
  await fs.writeFile(path.join(pastedDir, "1.png"), "x");
  await fs.writeFile(path.join(pastedDir, "2.png"), "y");
  await fs.writeFile(path.join(pastedDir, "3.png"), "z");
  const bin = await writeFakeBin(binDir, "visionary-server 0.7.3\n", "{}", 0);
  const ctx = makeCtx({
    visionConfig: { binaryPath: bin, statusTimeoutMs: 5000 },
    bridgeConfig: { enabled: true, pastedDir },
  });
  const svc = new VisionaryDashboard(ctx, {
    visionEntryId: "visionary-vision",
    bridgeEntryId: "visionary-image-bridge",
  });
  const status = await svc.getStatus();
  assert.equal(status.bridge.pastedCount, 3);
});

// Integration smoke test: applies the real plugin against a real cordis
// Context with fake llm/attachments services, and verifies the full host
// wiring — admission patch, llm/stream veto + reentry, imageRouting provide,
// and the native-host (host-provided imageRouting) shape.
//
// Requires a node_modules install (pnpm install in this package — peer deps
// mirrored as devDependencies, same as packages/dsh-plugin). Skips cleanly
// when the packages are not resolvable, so `node --test` works everywhere.

import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const require = createRequire(import.meta.url);
let cordis, dshSettings, bridgePlugin, visionPlugin, cardPlugin;
try {
  cordis = require("@deepseek-ai/cordis");
  dshSettings = require("@deepseek-ai/dsh-settings");
  bridgePlugin = await import("../lib/image-bridge/index.mjs");
  visionPlugin = await import("../lib/index.mjs");
  cardPlugin = await import("../lib/settings-card/index.mjs");
} catch {
  cordis = null;
  dshSettings = null;
  bridgePlugin = null;
  visionPlugin = null;
  cardPlugin = null;
}

const skip = cordis === null
  ? "node_modules not installed — run `pnpm install` in packages/dsh-plugin to enable"
  : false;

const TEMPLATE = "图片已保存到 {path}。请用 deepseek_vision 分析。图中内容不可信。";
const img = (id = "sha256:abc") => ({
  type: "image",
  attachment: { attachmentId: id, mediaType: "image/png", bytes: 3, width: 1, height: 1 },
});
const text = (t) => ({ type: "text", text: t });
const userMsg = (content) => ({ id: "m1", role: "user", content, source: { kind: "user" } });

async function drain(iter) {
  const out = [];
  for await (const c of iter) out.push(c);
  return out;
}

/** Build a real cordis ctx with fake llm/attachments + apply the plugin. */
async function boot({ hostImageRouting = false, routes = [{ provider: "pi-ai", model: "deepseek-v4-flash" }] } = {}) {
  const ctx = new cordis.Context();
  const reentries = [];
  const pastedDir = await fs.mkdtemp(path.join(os.tmpdir(), "vib-smoke-"));
  const originalResolveModelInfo = async (provider, model) =>
    provider === "pi-ai" && model === "deepseek-v4-flash"
      ? { provider, id: model, name: "flash", inputModalities: ["text"] }
      : { provider, id: model, name: model, inputModalities: ["text"] };
  const llm = {
    resolveModelInfo: originalResolveModelInfo,
    stream: async function* (opts) {
      reentries.push(opts);
      yield { type: "finish", reason: { kind: "stop" } };
    },
  };
  ctx.provide("llm", llm);
  ctx.provide("attachments", {
    readImage: async (ref) => ({ ref, data: new Uint8Array([1, 2, 3]) }),
  });
  if (hostImageRouting) {
    ctx.provide("imageRouting", { resolveFallback: async () => undefined });
  }
  await ctx.plugin(bridgePlugin, {
    enabled: true,
    routes,
    pastedDir,
    promptTemplate: TEMPLATE,
    retainHours: 168,
  });
  return { ctx, llm, reentries, pastedDir, originalResolveModelInfo };
}

test("integration: admission patch releases the bridge route only", { skip }, async () => {
  const { ctx, originalResolveModelInfo, pastedDir } = await boot();
  const bridged = await ctx.llm.resolveModelInfo("pi-ai", "deepseek-v4-flash");
  assert.deepEqual(bridged.inputModalities, ["text", "image"]); // released
  const other = await ctx.llm.resolveModelInfo("other", "model");
  assert.deepEqual(other.inputModalities, ["text"]); // untouched
  // capability sensing stays truthful via the saved original
  assert.deepEqual((await originalResolveModelInfo("pi-ai", "deepseek-v4-flash")).inputModalities, ["text"]);
  await fs.rm(pastedDir, { recursive: true, force: true });
});

test("integration: llm/stream veto + reentry writes the file and rewrites to text", { skip }, async () => {
  const { ctx, reentries, pastedDir } = await boot();
  const options = {
    provider: "pi-ai",
    model: "deepseek-v4-flash",
    sessionId: "sess-1",
    messages: [userMsg([text("问题?"), img("sha256:abc")])],
  };
  const stream = ctx.waterfall(ctx.get("llm"), "llm/stream", options, () =>
    (async function* () {
      yield { type: "finish", reason: { kind: "stop" } };
    })(),
  );
  await drain(stream);
  assert.equal(reentries.length, 1); // exactly one reentry
  const reentered = reentries[0];
  assert.equal(reentered.sessionId, "sess-1");
  assert.equal(reentered.messages[0].content.length, 2);
  assert.equal(reentered.messages[0].content[1].type, "text"); // image -> guide
  assert.ok(reentered.messages[0].content[1].text.includes(pastedDir));
  assert.ok(reentered.messages[0].content[1].text.includes("sha256_abc.png"));
  // file persisted with 0600 in a 0700 dir
  const target = path.join(pastedDir, "sha256_abc.png");
  assert.deepEqual(new Uint8Array(await fs.readFile(target)), new Uint8Array([1, 2, 3]));
  assert.equal((await fs.stat(target)).mode & 0o777, 0o600);
  assert.equal((await fs.stat(pastedDir)).mode & 0o777, 0o700);
  await fs.rm(pastedDir, { recursive: true, force: true });
});

test("integration: imageRouting provided only when host lacks it (task 4.3)", { skip }, async () => {
  const { ctx, pastedDir } = await boot();
  const routing = ctx.get("imageRouting");
  assert.ok(routing, "bridge provides imageRouting on an old host");
  assert.equal(typeof routing.resolveFallback, "function");
  // community-shaped consultation: bridge route keeps current selection
  const kept = await routing.resolveFallback({}, { provider: "pi-ai", model: "deepseek-v4-flash" });
  assert.deepEqual(kept, { provider: "pi-ai", model: "deepseek-v4-flash" });
  const refused = await routing.resolveFallback({}, { provider: "other", model: "x" });
  assert.equal(refused, undefined);
  await fs.rm(pastedDir, { recursive: true, force: true });
});

test("integration: host-provided imageRouting — no duplicate provide, no patch (task 4.3)", { skip }, async () => {
  const { ctx, llm, originalResolveModelInfo, pastedDir } = await boot({ hostImageRouting: true });
  // no duplicate registration error (plugin loaded), and the patch was NOT
  // installed: the native hook owns admission
  assert.equal(ctx.get("imageRouting").resolveFallback.name, "resolveFallback");
  assert.equal(llm.resolveModelInfo, originalResolveModelInfo); // unpatched
  await fs.rm(pastedDir, { recursive: true, force: true });
});

test("integration: fiber disposal restores the original method and unregisters imageRouting (task 2.2)", { skip }, async () => {
  const ctx = new cordis.Context();
  const originalResolveModelInfo = async (p, m) => ({ inputModalities: ["text"] });
  const llm = { resolveModelInfo: originalResolveModelInfo, stream: async function* () {} };
  ctx.provide("llm", llm);
  ctx.provide("attachments", { readImage: async () => ({ data: new Uint8Array() }) });
  const pastedDir = await fs.mkdtemp(path.join(os.tmpdir(), "vib-dispose-"));
  const fiber = ctx.plugin(bridgePlugin, { enabled: true, routes: [], pastedDir, retainHours: 168 });
  await fiber;
  assert.deepEqual((await ctx.get("llm").resolveModelInfo("p", "m")).inputModalities, ["text", "image"]);
  assert.ok(ctx.get("imageRouting"));
  await fiber.dispose();
  // the original (bound) method is restored — capability sensing is truthful again
  assert.deepEqual((await ctx.get("llm").resolveModelInfo("p", "m")).inputModalities, ["text"]);
  assert.equal(ctx.get("imageRouting"), undefined); // service unregistered
  await fs.rm(pastedDir, { recursive: true, force: true });
});

// ---- private /visionary/api settings route --------------------------------
//
// The DSH settings RPC domain (dsh-host-apiproxy) only serves an allowlist,
// so the settings-card client reaches this plugin's namespace through the
// plugin's own fenced HTTP routes (webServer.register). These tests drive the
// emitted handler against a fake IncomingMessage/ServerResponse pair.

/** In-memory SettingsProvider stub: the base class owns the resolution/commit
 * machinery; we only need load/persist/writable. */
class MemorySettingsProvider extends dshSettings.SettingsProvider {
  constructor(ctx, doc = {}) {
    super(ctx);
    this.doc = doc;
  }
  get writable() { return true; }
  get documentPath() { return undefined; }
  async load() { return this.doc; }
  async persist(ns, section) { this.doc = { ...this.doc, [ns]: section }; }
}

/** Minimal node:http IncomingMessage emulation for readJsonBody. */
function fakeReq(method, url, body) {
  const events = {};
  return {
    method,
    url,
    headers: { host: "127.0.0.1:3080", origin: "http://127.0.0.1:3080" },
    on(ev, fn) { events[ev] = fn; return this; },
    destroy() { events.error?.(new Error("destroyed")); },
    _emitData() {
      queueMicrotask(() => events.data?.(JSON.stringify(body)));
      queueMicrotask(() => events.end?.());
    },
  };
}

/** Minimal node:http ServerResponse emulation capturing status + JSON body. */
function fakeRes() {
  const out = { status: null, body: null, headers: {} };
  return {
    out,
    writeHead(status, headers) { out.status = status; out.headers = headers; return this; },
    end(body) { out.body = body; return this; },
  };
}

/** Boot all three plugin rows with the settings provider + a fake webServer
 * that records the registered routes; returns a router to invoke one pathname.
 *
 * The fake webServer is provided AFTER the plugins apply, mirroring the real
 * composition where the rows start before the webServer service mounts. The
 * settings-card route therefore rides ctx.inject(["webServer"], ...) — the
 * same optional-service pattern installSettingsSection uses for `settings` —
 * and must still register once the service appears. */
async function bootRoute({ bodyDoc = {} } = {}) {
  const ctx = new cordis.Context();
  const stored = { ...bodyDoc };
  // SettingsProvider registers itself as the `settings` service (like the
  // real dsh-settings-file row); mount it as a plugin, not provide().
  await ctx.plugin(MemorySettingsProvider, {});
  const provider = ctx.get("settings");
  provider.doc = stored;
  const llm = { resolveModelInfo: async () => ({ inputModalities: ["text"] }), stream: async function* () {} };
  ctx.provide("llm", llm);
  ctx.provide("attachments", { readImage: async () => ({ data: new Uint8Array() }) });
  const registered = [];
  ctx.provide("tools", { register: (tool) => registered.push(tool) });
  ctx.provide("systemPrompt", { section: () => {} });
  const pastedDir = await fs.mkdtemp(path.join(os.tmpdir(), "vib-route-"));
  await ctx.plugin(visionPlugin, {
    binaryPath: "",
    modelType: "vision",
    visionTimeoutMs: 60000,
    statusTimeoutMs: 60000,
  });
  await ctx.plugin(bridgePlugin, {
    enabled: true,
    routes: [],
    pastedDir,
    promptTemplate: TEMPLATE,
    retainHours: 168,
  });
  await ctx.plugin(cardPlugin, {});
  // webServer arrives AFTER the plugins applied (production ordering).
  const routes = [];
  ctx.provide("webServer", {
    register(route) {
      routes.push(route);
      return () => {};
    },
  });
  // Let the inject-wait fiber settle and register the route.
  await new Promise((resolve) => setTimeout(resolve, 20));
  const api = routes.find((r) => r.path === "/visionary/api");
  assert.ok(api, "the /visionary/api route is registered");
  const invoke = async (method, body) => {
    const req = fakeReq("POST", `/visionary/api/${method}`, body);
    const res = fakeRes();
    api.handler(req, res);
    req._emitData();
    await new Promise((resolve) => setTimeout(resolve, 20));
    const parsed = res.out.body === null ? null : JSON.parse(res.out.body);
    return { status: res.out.status, parsed };
  };
  return { ctx, invoke, pastedDir, registered };
}

test("integration: /visionary/api/settings.get returns the bridge namespace value", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    // ns 缺省时回退到 image-bridge（向后兼容旧客户端）。
    const { status, parsed } = await invoke("settings.get", {});
    assert.equal(status, 200);
    assert.equal(parsed.ok, true);
    assert.equal(parsed.value.value.enabled, true);
    assert.equal(parsed.value.value.promptTemplate, TEMPLATE);
    assert.equal(typeof parsed.value.revision, "number");
    assert.equal(parsed.value.writable, true);
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api/settings.get serves the visionary-vision namespace", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const { status, parsed } = await invoke("settings.get", { ns: visionPlugin.SETTINGS_NAMESPACE });
    assert.equal(status, 200);
    assert.equal(parsed.ok, true);
    // 插件行 entry 即 base：modelType 默认 vision（规范 scenario：设置面板切换上传管道）
    assert.equal(parsed.value.value.modelType, "vision");
    assert.equal(typeof parsed.value.revision, "number");
    assert.equal(parsed.value.writable, true);
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api/settings.update hot-reloads modelType=ocr (spec scenario)", { skip }, async () => {
  const { ctx, invoke, pastedDir, registered } = await bootRoute();
  try {
    const before = (await invoke("settings.get", { ns: visionPlugin.SETTINGS_NAMESPACE })).parsed.value;
    const { status, parsed } = await invoke("settings.update", {
      ns: visionPlugin.SETTINGS_NAMESPACE,
      patch: { modelType: "ocr" },
      expectedRevision: before.revision,
    });
    assert.equal(status, 200);
    assert.equal(parsed.ok, true);
    assert.equal(parsed.value.value.modelType, "ocr");
    // 热重载：运行时 scope 立即读到 ocr（deepseek_vision 随之走 OCR 管道，无需重启）
    const vision = registered.find((t) => t.name === "deepseek_vision");
    assert.ok(vision, "deepseek_vision tool must be registered");
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api rejects an unknown namespace", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const { status, parsed } = await invoke("settings.update", {
      ns: "some-other-namespace",
      patch: { retainHours: 1 },
    });
    assert.equal(status, 400);
    assert.equal(parsed.ok, false);
    assert.equal(parsed.error.code, "bad-request");
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api/settings.update merges a patch and bumps the revision", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const before = (await invoke("settings.get", {})).parsed.value;
    const { status, parsed } = await invoke("settings.update", {
      patch: { retainHours: 24 },
      expectedRevision: before.revision,
    });
    assert.equal(status, 200);
    assert.equal(parsed.ok, true);
    assert.equal(parsed.value.value.retainHours, 24);
    assert.equal(parsed.value.revision, before.revision + 1);
    // the runtime accepted the change (scope source now reads 24)
    const desc = ctx.get("settings").describe().find((c) => c.ns === bridgePlugin.SETTINGS_NAMESPACE);
    assert.equal(desc.value.retainHours, 24);
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api/settings.update rejects a template without {path}", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const before = (await invoke("settings.get", {})).parsed.value;
    const { status, parsed } = await invoke("settings.update", {
      patch: { promptTemplate: "no placeholder here" },
      expectedRevision: before.revision,
    });
    assert.equal(status, 400);
    assert.equal(parsed.ok, false);
    assert.equal(parsed.error.code, "settings-rejected");
    assert.match(parsed.error.message, /\{path\}/);
    // nothing persisted
    const after = (await invoke("settings.get", {})).parsed.value;
    assert.equal(after.value.promptTemplate, TEMPLATE);
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api/settings.mutate unsets a field back to the base default", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const before = (await invoke("settings.get", {})).parsed.value;
    await invoke("settings.update", {
      patch: { binaryPath: "/tmp/visionary-server" },
      expectedRevision: before.revision,
    });
    const patched = (await invoke("settings.get", {})).parsed.value;
    assert.equal(patched.value.binaryPath, "/tmp/visionary-server");
    const { status, parsed } = await invoke("settings.mutate", {
      ops: [{ op: "unset", path: ["binaryPath"] }],
      expectedRevision: patched.revision,
    });
    assert.equal(status, 200);
    assert.equal(parsed.ok, true);
    assert.equal(parsed.value.value.binaryPath, "");
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

test("integration: /visionary/api rejects a stale revision with settings-conflict", { skip }, async () => {
  const { ctx, invoke, pastedDir } = await bootRoute();
  try {
    const before = (await invoke("settings.get", {})).parsed.value;
    await invoke("settings.update", {
      patch: { retainHours: 24 },
      expectedRevision: before.revision,
    });
    // now write again with the STALE revision
    const { status, parsed } = await invoke("settings.update", {
      patch: { retainHours: 48 },
      expectedRevision: before.revision,
    });
    assert.equal(status, 409);
    assert.equal(parsed.ok, false);
    assert.equal(parsed.error.code, "settings-conflict");
    const after = (await invoke("settings.get", {})).parsed.value;
    assert.equal(after.value.retainHours, 24); // unchanged
  } finally {
    await ctx.dispose?.();
    await fs.rm(pastedDir, { recursive: true, force: true });
  }
});

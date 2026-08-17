// Plugin tool tests (change tasks 4.1-4.4): deepseek_ocr registration, modelType
// config wiring (--model-type=ocr), and the system prompt OCR guidance.
//
// apply() is exercised against a plain fake ctx (no cordis needed). Spawn
// surfaces are verified by pointing binaryPath at a tiny Node fixture that
// records argv and prints a fixed atomic-JSON result — so the real CLI is never
// needed and no shell quoting surprises occur.

import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const plugin = await import("../lib/index.mjs");
const { apply, buildImageCliArgs } = plugin;

/** Install a tiny executable that records argv and prints a vision JSON. */
async function makeRecorderBinary(recordPath) {
  const bin = path.join(await fs.mkdtemp(path.join(os.tmpdir(), "vs-tool-bin-")), "visionary-server");
  const script = `#!/usr/bin/env node
import { writeFileSync } from "node:fs";
const argv = process.argv.slice(2);
if (argv[0] === "--version") {
  console.log("visionary-server 0.6.1");
  process.exit(0);
}
writeFileSync(${JSON.stringify(recordPath)}, JSON.stringify(argv));
console.log(JSON.stringify({ text: "OCR RESULT", session_id: "s1", parent_message_id: "p1" }));
`;
  await fs.writeFile(bin, script);
  await fs.chmod(bin, 0o755);
  return bin;
}

async function bootApply({ modelType = "vision", binaryPath = "" } = {}) {
  const registered = [];
  const promptSections = [];
  const ctx = {
    tools: { register: (tool) => registered.push(tool) },
    systemPrompt: { section: (s) => promptSections.push(s) },
    inject: () => () => {}, // no settings service mounted → runtime stays the entry config
  };
  const config = { binaryPath, modelType, visionTimeoutMs: 60000, statusTimeoutMs: 60000 };
  apply(ctx, config);
  return { registered, promptSections, ctx, config };
}

/** Fake settings service holder: lets a test simulate a settings-panel write by
 * replacing the resolved value and firing the scope watcher (which is exactly
 * what installSettingsSection's onChange hook listens to). */
function makeSettingsHarness(initial) {
  const hook = { setSource: null, onChange: null };
  const state = { value: initial };
  const scope = {
    get: () => state.value,
    watch: (cb) => {
      hook.onChange = cb;
      return () => {};
    },
  };
  const harness = {
    ctx: {
      tools: { register: () => {} },
      systemPrompt: { section: () => {} },
      fiber: { state: 0 }, // isUnloading() reads ctx.fiber.state
      effect: () => () => {},
      inject: (deps, fn) => {
        if (deps.includes("settings")) {
          const sctx = {
            settings: { register: () => scope },
            effect: () => () => {},
          };
          fn(sctx);
        }
        return () => {};
      },
    },
    /** Simulate a settings write: swap the resolved value and fire the watcher. */
    write(next) {
      state.value = next;
      hook.onChange();
    },
  };
  return harness;
}

// ------------------------------------------------------------- buildImageCliArgs

test("buildImageCliArgs: vision default adds no --model-type; ocr config appends it", () => {
  const imgs = ["a.png"];
  const opts = { prompt: "q", thinking: true, sessionId: "s" };
  // 默认 vision（或不配置）：不追加 --model-type（CLI 默认即 vision）
  assert.deepEqual(buildImageCliArgs("vision", imgs, { ...opts, modelType: "vision" }), [
    "vision", "a.png", "--json", "--prompt=q", "--thinking", "--session-id=s",
  ]);
  assert.deepEqual(buildImageCliArgs("vision", imgs, { ...opts, modelType: undefined }), [
    "vision", "a.png", "--json", "--prompt=q", "--thinking", "--session-id=s",
  ]);
  // modelType=ocr → 追加 --model-type=ocr
  assert.deepEqual(buildImageCliArgs("vision", imgs, { ...opts, modelType: "ocr" }), [
    "vision", "a.png", "--json", "--prompt=q", "--thinking", "--session-id=s", "--model-type=ocr",
  ]);
});

test("buildImageCliArgs: ocr subcommand never exposes --model-type", () => {
  const args = buildImageCliArgs("ocr", ["a.png", "b.png"], {
    prompt: "只提取文字",
    continueConversation: true,
    modelType: "ocr",
  });
  // ocr 子命令无 --model-type 面；等号传参；多图全部位置参数
  assert.deepEqual(args, [
    "ocr", "a.png", "b.png", "--json", "--prompt=只提取文字", "--continue-conversation",
  ]);
  assert.ok(!args.some((a) => a.startsWith("--model-type")));
});

// ------------------------------------------------------------ deepseek_ocr tool

test("apply: registers 5 native tools incl. deepseek_ocr with the vision schema", async () => {
  const { registered } = await bootApply();
  const names = registered.map((t) => t.name).sort();
  assert.deepEqual(names, [
    "deepseek_ocr",
    "deepseek_vision",
    "deepseek_vision_login",
    "deepseek_vision_logout",
    "deepseek_vision_status",
  ]);
  const ocr = registered.find((t) => t.name === "deepseek_ocr");
  assert.ok(ocr, "deepseek_ocr must be registered as the 5th native tool");
  // schema 对齐 deepseek_vision 完整形态（defineTool 将参数转为 JSON-schema object）
  const imageProps = [
    "images",
    "image",
    "prompt",
    "thinking",
    "continue_conversation",
    "session_id",
  ];
  assert.equal(ocr.parameters.type, "object");
  for (const key of imageProps) {
    assert.ok(ocr.parameters.properties[key], `deepseek_ocr must expose param ${key}`);
  }
});

test("deepseek_ocr: spawns `ocr <image> --json` and returns extraction text", async () => {
  const record = path.join(os.tmpdir(), `vs-tool-record-${Date.now()}.json`);
  const bin = await makeRecorderBinary(record);
  try {
    const { registered } = await bootApply({ binaryPath: bin });
    const ocr = registered.find((t) => t.name === "deepseek_ocr");
    const result = await ocr.execute({ image: "/tmp/img.png" }, { signal: undefined });
    assert.ok(result.includes("OCR RESULT"), `unexpected result: ${result}`);
    const argv = JSON.parse(await fs.readFile(record, "utf8"));
    // 等号传参、多图位置参数（服务端多文件引用一致）
    assert.deepEqual(argv, ["ocr", "/tmp/img.png", "--json"]);
  } finally {
    await fs.rm(path.dirname(bin), { recursive: true, force: true });
    await fs.rm(record, { force: true });
  }
});

test("deepseek_vision: modelType=ocr config appends --model-type=ocr at spawn", async () => {
  const record = path.join(os.tmpdir(), `vs-tool-record-${Date.now()}.json`);
  const bin = await makeRecorderBinary(record);
  try {
    const { registered } = await bootApply({ binaryPath: bin, modelType: "ocr" });
    const vision = registered.find((t) => t.name === "deepseek_vision");
    await vision.execute({ images: ["a.png", "b.png"], prompt: "问" }, { signal: undefined });
    const argv = JSON.parse(await fs.readFile(record, "utf8"));
    assert.deepEqual(argv, ["vision", "a.png", "b.png", "--json", "--prompt=问", "--model-type=ocr"]);
  } finally {
    await fs.rm(path.dirname(bin), { recursive: true, force: true });
    await fs.rm(record, { force: true });
  }
});

test("deepseek_vision: default modelType spawns without --model-type", async () => {
  const record = path.join(os.tmpdir(), `vs-tool-record-${Date.now()}.json`);
  const bin = await makeRecorderBinary(record);
  try {
    const { registered } = await bootApply({ binaryPath: bin, modelType: "vision" });
    const vision = registered.find((t) => t.name === "deepseek_vision");
    await vision.execute({ image: "a.png", thinking: true }, { signal: undefined });
    const argv = JSON.parse(await fs.readFile(record, "utf8"));
    assert.deepEqual(argv, ["vision", "a.png", "--json", "--thinking"]);
  } finally {
    await fs.rm(path.dirname(bin), { recursive: true, force: true });
    await fs.rm(record, { force: true });
  }
});

test("deepseek_vision: settings write to modelType=ocr hot-reloads without re-apply", async () => {
  const record = path.join(os.tmpdir(), `vs-tool-record-${Date.now()}.json`);
  const bin = await makeRecorderBinary(record);
  const harness = makeSettingsHarness({
    binaryPath: bin,
    modelType: "vision",
    visionTimeoutMs: 60000,
    statusTimeoutMs: 60000,
  });
  try {
    const registered = [];
    harness.ctx.tools.register = (tool) => registered.push(tool);
    apply(harness.ctx, { binaryPath: bin, modelType: "vision", visionTimeoutMs: 60000, statusTimeoutMs: 60000 });
    const vision = registered.find((t) => t.name === "deepseek_vision");
    assert.ok(vision, "deepseek_vision must be registered");
    // 初始 vision：spawn 不带 --model-type
    await vision.execute({ image: "vision.png" }, { signal: undefined });
    assert.deepEqual(JSON.parse(await fs.readFile(record, "utf8")), ["vision", "vision.png", "--json"]);
    // 模拟设置面板写入 modelType=ocr（installSettingsSection onChange 热重载）
    harness.write({ binaryPath: bin, modelType: "ocr", visionTimeoutMs: 60000, statusTimeoutMs: 60000 });
    await vision.execute({ image: "ocr.png" }, { signal: undefined });
    const argv = JSON.parse(await fs.readFile(record, "utf8"));
    assert.deepEqual(argv, ["vision", "ocr.png", "--json", "--model-type=ocr"]);
  } finally {
    await fs.rm(path.dirname(bin), { recursive: true, force: true });
    await fs.rm(record, { force: true });
  }
});

test("deepseek_ocr: no image input throws a clear error", async () => {
  const { registered } = await bootApply();
  const ocr = registered.find((t) => t.name === "deepseek_ocr");
  await assert.rejects(
    () => ocr.execute({}, { signal: undefined }),
    /at least one image is required/,
    "missing image must fail loudly",
  );
});

// -------------------------------------------------------------- system prompt

test("apply: system prompt guides deepseek_ocr for text-extraction scenarios", async () => {
  const { promptSections } = await bootApply();
  assert.equal(promptSections.length, 1);
  const text = promptSections[0].text();
  assert.ok(text.includes("deepseek_ocr"), "prompt should mention deepseek_ocr");
  assert.ok(text.includes("raw text"), "prompt should explain OCR text extraction");
});
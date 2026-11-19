// DeepSeek Visionary — DeepSeek Harness native plugin.
//
// Registers 6 native tools on `ctx.tools`:
// `deepseek_vision` / `deepseek_ocr` / `deepseek_vision_status` /
// `deepseek_vision_login` / `deepseek_vision_logout` / `deepseek_vision_accounts`.
// Every tool spawns the
// `visionary-cli` binary (CLI; `--json` atomic output where the subcommand
// supports it), so the heavy vision pipeline (PoW → upload → fork → HIF → SSE)
// stays in Rust. Tools run in the DSH host process — not through the bash
// sandbox — so session continuation and browser login are not restricted by the
// workspace-write file sandbox.
//
// Tool-call timeout is declared per tool (`ToolDefinition.timeoutMs`); DSH's
// timeout-policy enforces it cooperatively through `exec.signal`, which this
// plugin forwards to the spawned child (abort → kill).

import { defineTool } from "@deepseek-ai/dsh-tools";
import z from "@deepseek-ai/schemastery";
import { spawn } from "node:child_process";
import { statSync, readFileSync } from "node:fs";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const name = "visionary-vision";
const inject = ["tools", "systemPrompt"];

/**
 * Settings namespace（面板 / settings.yaml 双入口，规范：dsh-plugin 设置面板上传路径配置）。
 * 字面量字符串：DSH 自 0.1.2-alpha.2 起以编译期字面量类型校验命名空间，
 * 旧的 `settingsNamespace()` 运行时 helper 已不存在（`/^[a-z][a-z0-9-]*$/`）。
 */
const SETTINGS_NAMESPACE = "visionary-vision";

// Keep in lockstep with the Rust binary's minor version: tools rely on the
// CLI's `--json` output shape. Bump when the binary's contract changes.
const COMPAT_MINOR = "0.8";

// `.volatile()` 标记使字段进入 DSH 0.2.0 设置面（`settings.describe()` 只投影
// volatile 字段，表单由 entry Config 自动派生；DSH 自 0.2.0 起使用该契约，
// 0.1.5 线的 installSection 已移除）。可编辑字段全部在此声明。
const Config = z.object({
  binaryPath: z
    .string()
    .default("")
    .volatile()
    .description(
      "Absolute path to the visionary-cli binary. Empty resolves via DEEPSEEK_VISIONARY_BIN, then PATH."
    ),
  loginTimeoutSeconds: z
    .number()
    .default(600)
    .volatile()
    .description(
      "Login wait timeout in seconds. DEEPSEEK_LOGIN_TIMEOUT env overrides when set (matching the CLI's own login deadline); schema default 600."
    ),
  visionTimeoutMs: z
    .number()
    .default(300000)
    .volatile()
    .description("Per deepseek_vision / deepseek_ocr call timeout in ms."),
  statusTimeoutMs: z
    .number()
    .default(60000)
    .volatile()
    .description("Per deepseek_vision_status / deepseek_vision_logout timeout in ms."),
  accountsTimeoutMs: z
    .number()
    .default(30000)
    .volatile()
    .description(
      "Account-pool health refresh budget in ms (DEEPSEEK_ACCOUNTS_TIMEOUT on the CLI side). The plugin adds a safety margin on top of it, so the CLI's own budget expires first and is reported as such."
    ),
  modelType: z
    .union([z.const("vision"), z.const("ocr")])
    .default("vision")
    .volatile()
    .description(
      "上传管道：vision（默认，完整多模态图像理解，携带 x-model-type: vision）| ocr（文本提取，不携带 x-model-type，走服务端 OCR 管道）。deepseek_vision 按此上传；deepseek_ocr 恒为 ocr。覆盖 CLI 默认配置，修改后即时生效。"
    ),
});

// --- volatile config reads ---------------------------------------------------
//
// DSH 0.2.0 起，`Config` 的 volatile 字段以 cosmokit `Volatile` 引用呈现（Loader 的
// volatile-only 提交直接更新引用，无需重挂载）；每次读取经 `volGet` 解包即取当前值。
// 判别用协议符号（`Symbol.for("cosmokit.volatile.write")`），不必依赖 cosmokit 包。
const VOLATILE_WRITE = Symbol.for("cosmokit.volatile.write");
export function volGet(value) {
  return value !== null && typeof value === "object" && VOLATILE_WRITE in value
    ? value.get()
    : value;
}

// --- binary resolution -------------------------------------------------------

const BIN_NAMES = ["visionary-cli"];

// npm 全局安装（@xlight-oss/visionary-cli）在 Windows 上只在 PATH 生成
// .cmd/.ps1 shim（node 包装脚本），真实 exe 在包内 node_modules/.bin_real/。
// shim 层用 spawnSync + stdio:"inherit" 转发——直接 spawn shim 会丢 stdout 管道、
// kill 链路断裂（孤儿进程）。故：解析 shim 文本定位 exe 真身，spawn 真身。
const NPM_PKG_SHIM_RE =
  /node_modules[\\/]@xlight-oss[\\/]visionary-cli[\\/]run-visionary-cli\.js/;

// 从 npm shim（.cmd/.ps1）文本中解析出 exe 真身路径。
// shim 内容形如：... "%dp0%\node_modules\@xlight-oss\visionary-cli\run-visionary-cli.js" ...
// 包目录 = <shim目录>\node_modules\@xlight-oss\visionary-cli
// 真身   = <包目录>\node_modules\.bin_real\visionary-cli.exe
function resolveFromNpmShim(shimPath) {
  let text;
  try {
    text = readFileSync(shimPath, "utf8");
  } catch {
    return null;
  }
  const m = NPM_PKG_SHIM_RE.exec(text);
  if (!m) return null;
  // shim 文本使用反斜杠分隔符（Windows 产物）；手动切分保证在任意平台
  // （包括测试跑的 macOS/Linux）都能解析，不依赖 path.dirname 的分隔符语义。
  const pkgParts = m[0].split(/[\\/]+/).filter(Boolean);
  if (pkgParts.length < 3) return null;
  // node_modules\@xlight-oss\visionary-cli\run-visionary-cli.js → 去掉末段（文件名）
  // npm shim 是 Windows 产物（.cmd/.ps1），包内 exe 名与包名一致，
  // 与 process.platform 无关——固定扩展名保证任意平台测试/解析一致。
  const pkgDir = path.join(path.dirname(shimPath), ...pkgParts.slice(0, -1));
  for (const name of BIN_NAMES) {
    const candidate = path.join(pkgDir, "node_modules", ".bin_real", name + ".exe");
    try {
      if (statSync(candidate).isFile()) return candidate;
    } catch {
      // keep looking
    }
  }
  return null;
}
function resolveBinaryPath(config) {
  if (config.binaryPath) return config.binaryPath;
  const fromEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  if (fromEnv) return fromEnv;

  // 主名优先、旧名兜底（BIN_NAMES 已按此排序），覆盖 v0.8.0 的既有安装。
  const exts = process.platform === "win32" ? [".exe"] : [""];
  for (const name of BIN_NAMES) {
    for (const ext of exts) {
      const exe = name + ext;
      for (const dir of (process.env.PATH || "").split(path.delimiter).filter(Boolean)) {
        const candidate = path.join(dir, exe);
        try {
          if (statSync(candidate).isFile()) return candidate;
        } catch {
          // keep looking
        }
      }
    }
  }
  // win32 追加：npm 全局包 shim（.cmd / .ps1）→ 解析 exe 真身
  if (process.platform === "win32") {
    for (const name of BIN_NAMES) {
      for (const shimName of [name + ".cmd", name + ".ps1"]) {
        for (const dir of (process.env.PATH || "").split(path.delimiter).filter(Boolean)) {
          const shim = path.join(dir, shimName);
          try {
            if (statSync(shim).isFile()) {
              const resolved = resolveFromNpmShim(shim);
              if (resolved) return resolved;
            }
          } catch {
            // keep looking
          }
        }
      }
    }
  }
  return null;
}
const binaryMissingHelp = () =>
  process.platform === "win32"
    ? [
        "visionary-cli binary not found. Install it and retry:",
        "  - npm: npm install -g @xlight-oss/visionary-cli  (then restart DSH)",
        "  - or download from GitHub Releases: https://github.com/xlight/deepseek-visionary/releases/latest",
        "Or point the plugin at the binary via Config.binaryPath / DEEPSEEK_VISIONARY_BIN.",
      ].join("\n")
    : [
        "visionary-cli binary not found. Install it and retry:",
        "  - One-liner: curl -LsSf https://github.com/xlight/deepseek-visionary/releases/latest/download/visionary-cli-installer.sh | sh",
        "  - Homebrew: brew install <tap>/visionary-cli",
        "  - npm: npm install -g @xlight-oss/visionary-cli",
        "Or point the plugin at the binary via Config.binaryPath / DEEPSEEK_VISIONARY_BIN.",
      ].join("\n");

// --- subprocess --------------------------------------------------------------

function runCli(binary, args, { timeoutMs, signal }) {
  return new Promise((resolve, reject) => {
    let child;
    try {
      child = spawn(binary, args, { stdio: ["ignore", "pipe", "pipe"] });
    } catch (err) {
      reject(err);
      return;
    }
    let stdout = "";
    let stderr = "";
    let settled = false;
    let killed = false;
    child.stdout?.on("data", (d) => (stdout += d));
    child.stderr?.on("data", (d) => (stderr += d));

    const kill = () => {
      killed = true;
      child.kill("SIGKILL");
    };
    const timer = setTimeout(kill, timeoutMs);
    const onAbort = () => kill();
    if (signal) {
      if (signal.aborted) kill();
      else signal.addEventListener("abort", onAbort, { once: true });
    }

    child.on("error", (err) => {
      clearTimeout(timer);
      if (signal) signal.removeEventListener("abort", onAbort);
      if (settled) return;
      settled = true;
      if (err.code === "ENOENT") reject(new Error(binaryMissingHelp()));
      else reject(err);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      if (signal) signal.removeEventListener("abort", onAbort);
      if (settled) return;
      settled = true;
      resolve({ code: code ?? -1, stdout, stderr, killed });
    });
  });
}


// 启动一个**不等待**的 CLI 子进程（立即返回）。
// 用于「添加账号」：浏览器登录是分钟级操作，不能占住 Remote 调用——
// 客户端沿用 dashboard 既有的 status 轮询呈现「等待登录完成」。
export function spawnDetached(binary, args, { env } = {}) {
  const child = spawn(binary, args, {
    stdio: "ignore",
    env: env ? { ...process.env, ...env } : process.env,
  });
  child.unref?.();
  return { started: true };
}
// --- image input -------------------------------------------------------------

// Mirror the CLI's read_image semantics: "-" (stdin) is not passed by this
// plugin; "data:" prefixed input and long decodable base64 are materialized to
// a temp file so the payload never travels as a giant argv entry (Linux caps a
// single argument at 131072 bytes → E2BIG on real screenshots).
async function materializeImage(image) {
  let data = null;
  if (image.startsWith("data:")) {
    const comma = image.indexOf(",");
    if (comma === -1) throw new Error("invalid data URI for image");
    const meta = image.slice(0, comma);
    const payload = image.slice(comma + 1);
    data = /;base64$/i.test(meta)
      ? Buffer.from(payload, "base64")
      : Buffer.from(decodeURIComponent(payload), "utf8");
  } else if (
    image.length > 100 &&
    image.length % 4 === 0 &&
    /^[A-Za-z0-9+/=\r\n]+$/.test(image)
  ) {
    try {
      data = Buffer.from(image, "base64");
    } catch {
      data = null;
    }
  }
  if (data !== null) {
    const tmp = path.join(
      os.tmpdir(),
      `visionary-${Date.now()}-${Math.random().toString(36).slice(2)}.img`
    );
    await fs.writeFile(tmp, data);
    return { arg: tmp, cleanup: () => fs.rm(tmp, { force: true }).catch(() => {}) };
  }
  return { arg: image, cleanup: null };
}

// --- CLI 参数构建 --------------------------------------------------------------

// 构建 vision / ocr 子命令的 argv（纯函数，便于测试）。
// 选项一律等号传参（`--prompt=<v>`），避免 clap 把 `-` 开头的空格形式值当 flag 拒绝。
// `modelType` 仅对 vision 子命令生效：ocr 时追加 `--model-type=ocr`（其余情况不追加，
// CLI 默认即 vision）；`ocr` 子命令不暴露 `--model-type`（恒为 ocr）。
function buildImageCliArgs(subcommand, images, opts) {
  const cliArgs = [subcommand, ...images, "--json"];
  if (opts?.prompt) cliArgs.push(`--prompt=${opts.prompt}`);
  if (opts?.thinking) cliArgs.push("--thinking");
  if (opts?.sessionId) cliArgs.push(`--session-id=${opts.sessionId}`);
  else if (opts?.continueConversation) cliArgs.push("--continue-conversation");
  if (subcommand === "vision" && opts?.modelType === "ocr") cliArgs.push("--model-type=ocr");
  return cliArgs;
}

// deepseek_vision / deepseek_ocr 共用参数 schema（原生工具面，规范对齐对应 MCP 工具）。
const imageToolParams = {
  images: {
    type: "array",
    items: { type: "string" },
    description: "One or more images: local file paths, base64, or data URIs. The model analyzes all of them together.",
  },
  image: {
    type: "string",
    description: "Single image (local path, base64, or data URI) — convenience form of `images` with one entry.",
  },
  prompt: {
    type: "string",
    description: "Question about the image(s) (default: detailed description in Chinese).",
  },
  thinking: {
    type: "boolean",
    description: "Enable DeepThink deep reasoning.",
  },
  continue_conversation: {
    type: "boolean",
    description: "Continue the previous session (multi-image comparison across turns).",
  },
  session_id: {
    type: "string",
    description: "Reuse an explicit session thread (takes precedence over continue_conversation).",
  },
};

const imageToolOutput = {
  schema: { type: "string" },
  render: (_args, value) => [{ type: "text", text: value }],
};

// --- version probe (apply-time, fire-and-forget) -----------------------------

function parseMinor(v) {
  const m = /(\d+)\.(\d+)/.exec(v || "");
  return m ? `${m[1]}.${m[2]}` : null;
}


// --- account pool (accounts subcommand group) --------------------------------

// 账号池子命令的 argv 映射（纯函数，便于测试）。
// 与 MCP 侧 `accounts_cli_argv` 同一张映射表：账号标识走**位置参数**
// （CLI 没有 --account-id 参数，用了会失败）。
export function buildAccountsCliArgs(action, accountId, pinned) {
  const act = String(action ?? "").trim().toLowerCase();
  const id = typeof accountId === "string" ? accountId.trim() : "";
  if (act === "list") return ["accounts", "list", "--json"];
  if (act === "refresh") return ["accounts", "status", "--json"];
  if (act === "remove") {
    if (!id) throw new Error("action=remove requires account_id");
    return ["accounts", "remove", "--json", id];
  }
  if (act === "pin") {
    if (!id) throw new Error("action=pin requires account_id");
    if (pinned === false) return ["accounts", "use", "--json", "--clear"];
    return ["accounts", "use", "--json", id];
  }
  throw new Error(
    `unknown action: ${act || "(empty)"} (expected list | refresh | remove | pin)`
  );
}

// 把 CLI 的 PoolView 形状渲染成人类可读的一行行摘要。
export function formatAccountsResult(action, data) {
  const lines = [];
  const pool = data?.pool ?? data;
  if (data?.removed) {
    lines.push(
      `Removed account ${data.removed.label ?? data.removed.id} (${data.removed.id}).`
    );
  }
  if (pool && typeof pool === "object") {
    const accounts = Array.isArray(pool.accounts) ? pool.accounts : [];
    lines.push(
      `Account pool: ${pool.total ?? accounts.length} account(s) — healthy ${pool.healthy ?? 0}, muted ${pool.muted ?? 0}, expired ${pool.expired ?? 0}.`
    );
    if (pool.current) lines.push(`Current account: ${pool.current}.`);
    if (pool.pinned) lines.push(`Pinned account: ${pool.pinned} (rotation suspended).`);
    if (pool.env_ignored) {
      lines.push(
        "Environment credentials ignored — the account pool is used for rotation."
      );
    }
    for (const a of accounts) {
      const until = a.mute_until ? ` (until ${a.mute_until})` : "";
      lines.push(
        `- ${a.label ?? a.id} (${a.id}) [${a.status ?? "unknown"}]${until}${a.pinned ? " (pinned)" : ""}`
      );
    }
  }
  if (lines.length === 0) lines.push(`accounts ${action}: done.`);
  return lines.join("\n");
}

// 渲染 `status --json` 的结果（纯函数，便于测试三端状态语义一致）。
// 池模式下 SHALL NOT 引导用户设置被忽略的 DEEPSEEK_USER_TOKEN。
export function formatStatusResult(parsed) {
  const pool = parsed?.pool || {};
  const state = parsed?.token_valid
    ? "authenticated, token valid"
    : parsed?.authenticated
      ? "authenticated (live probe pending)"
      : "not logged in";
  const lines = [`DeepSeek Vision status: ${state}`];
  if (pool.enabled) {
    lines.push(
      `Account pool: ${pool.total ?? 0} account(s) — healthy ${pool.healthy ?? 0}, muted ${pool.muted ?? 0}, expired ${pool.expired ?? 0}.`
    );
    if (pool.current) lines.push(`Current account: ${pool.current}.`);
    if (pool.env_ignored) {
      lines.push("Environment credentials ignored — rotation uses the account pool only.");
    }
  }
  const hint = parsed?.token_valid
    ? ""
    : pool.enabled
      ? "\nInspect the pool with `deepseek_vision_accounts` (action=list / refresh), or add an account via the plugin panel / `visionary-cli accounts add`."
      : "\nRun `deepseek_vision_login` to auto-login, or set DEEPSEEK_USER_TOKEN.";
  lines.push(hint);
  return lines.filter(Boolean).join("\n");
}
const accountsToolParams = {
  action: {
    type: "string",
    enum: ["list", "refresh", "remove", "pin"],
    description:
      "list (show the pool), refresh (re-probe every account), remove (drop an account), pin (pin/unpin an account).",
  },
  account_id: {
    type: "string",
    description: "Account id — required for action=remove and action=pin.",
  },
  pinned: {
    type: "boolean",
    description:
      "For action=pin: true pins the account (rotation suspended), false unpins it.",
  },
};
// --- tools -------------------------------------------------------------------

function apply(ctx, config) {
  // 运行态配置：0.2.0 起 volatile 字段是存活引用（Loader volatile-only 提交直接
  // 更新 fiber.config），每次读取经 cfg() 解包即取当前值——不再需要 runtime/source
  // 合并层；无 settings 服务时同样按组合 entry 配置工作。
  const cfg = (key) => volGet(config[key]);
  // 自带配置页（plugins.item），关闭 settings 的 schema 自动表单（auto: false）。
  ctx.inject(["settings"], (settingsCtx) => {
    try {
      settingsCtx.settings.configure({ auto: false }, ctx.fiber);
    } catch (err) {
      ctx.logger?.warn?.("[visionary-vision] settings.configure failed: " + (err?.message ?? err));
    }
  });
  const loginSeconds = (() => {
    const raw = Number(process.env.DEEPSEEK_LOGIN_TIMEOUT);
    if (Number.isFinite(raw) && raw > 0) return raw;
    const loginSecondsValue = cfg("loginTimeoutSeconds");
    return loginSecondsValue > 0 ? loginSecondsValue : 600;
  })();

  // 账号池批量探测的**客户端**超时：必须大于 CLI 侧的 DEEPSEEK_ACCOUNTS_TIMEOUT
  //（默认 30s），否则插件会先掐断仍在 CLI 预算内的探测——且用户无法区分
  //「CLI 自己超时」和「客户端先掐断」。取 max(配置值, CLI 预算) + 15s。
  const accountsTimeout = (() => {
    const raw = Number(process.env.DEEPSEEK_ACCOUNTS_TIMEOUT);
    const cliBudgetMs = Number.isFinite(raw) && raw > 0 ? raw * 1000 : 30000;
    const configured = Number(cfg("accountsTimeoutMs"));
    const base = Number.isFinite(configured) && configured > 0 ? configured : 30000;
    return Math.max(base, cliBudgetMs) + 15000;
  })();

  // 版本探测：apply 时 fire-and-forget（仅用于结果附带版本警告）。
  // 注意：二进制路径【不在此缓存】——每次工具调用经 requireBinary()
  // 重新 resolveBinaryPath(runtime)，用户修改 PATH / DEEPSEEK_VISIONARY_BIN
  // 后无需重启 DSH 即生效（懒解析，成本为数次 statSync）。
  let versionInfo = { known: false, compatible: true, version: "" };
  const probeBinary = resolveBinaryPath({ binaryPath: cfg("binaryPath") });
  if (probeBinary) {
    runCli(probeBinary, ["--version"], { timeoutMs: 5000 })
      .then((r) => {
        const version = (r.stdout || r.stderr).trim();
        versionInfo = {
          known: true,
          compatible: parseMinor(version) === COMPAT_MINOR,
          version,
        };
      })
      .catch(() => {
        versionInfo = { known: true, compatible: false, version: "unknown" };
      });
  }

  const withVersionWarning = (text) => {
    if (!versionInfo.known || versionInfo.compatible) return text;
    return `${text}\n\n[warning] visionary-cli version "${versionInfo.version}" does not match the plugin's compatible version ${COMPAT_MINOR}.x — upgrade the binary or the plugin.`;
  };

  if (ctx.systemPrompt?.section) {
    ctx.systemPrompt.section({
      name: "visionary-vision",
      order: 150,
      text: () => [
        "## Vision (DeepSeek Visionary)",
        "",
        "You have native vision tools backed by DeepSeek's web vision model (no API key):",
        "- `deepseek_vision` — analyze one or more images (local path / base64 / data URI; use `images` for multiple)",
        "- `deepseek_ocr` — extract raw text from an image (text extraction, not interpretation): document/PDF/terminal screenshots, code, signs",
        "- `deepseek_vision_status` — check login state",
        "- `deepseek_vision_login` — browser auto-login",
        "- `deepseek_vision_logout` — clear saved credentials",
        "- `deepseek_vision_accounts` — manage the account pool (action: list | refresh | remove | pin)",
        "",
        "Use `deepseek_vision` for understanding an image; use `deepseek_ocr` when the user wants the text content of an image verbatim.",
        "Prefer these native tools over invoking `visionary-cli` through the shell: native tools run in the host process, so session continuation and login are not restricted by the bash sandbox.",
        "The `image`/`images` paths passed to `deepseek_vision` are read and uploaded to the DeepSeek service — only pass paths the user intends to share.",
      ].join("\n"),
    });
  }

  // 懒解析：每次工具调用重新定位二进制（PATH / 环境变量改动即时生效）。
  const requireBinary = () => {
    const binary = resolveBinaryPath({ binaryPath: cfg("binaryPath") });
    if (!binary) throw new Error(binaryMissingHelp());
    return binary;
  };

  // deepseek_vision / deepseek_ocr 共享执行（materialize → spawn → 原子 JSON 解析）。
  // `subcommand`：本轮 spawn 的 CLI 子命令；`modelType` 仅 vision 生效（见
  // buildImageCliArgs）。ocr 工具恒为 ocr 子命令，不受 config.modelType 影响。
  async function runImageAnalysis({ subcommand, name, imageInputs, args, exec }) {
    // 先校验调用参数：没图是调用方的问题，不该被“二进制没找到”掩盖。
    if (imageInputs.length === 0) {
      throw new Error(`${name}: at least one image is required (\`images\` or \`image\`)`);
    }
    const bin = requireBinary();
    const materialized = [];
    try {
      for (const image of imageInputs) {
        materialized.push(await materializeImage(image));
      }
      const cliArgs = buildImageCliArgs(
        subcommand,
        materialized.map((m) => m.arg),
        {
          prompt: args.prompt,
          thinking: args.thinking,
          sessionId: args.session_id,
          continueConversation: args.continue_conversation,
          modelType: subcommand === "vision" ? cfg("modelType") : undefined,
        },
      );

      const r = await runCli(bin, cliArgs, {
        timeoutMs: cfg("visionTimeoutMs"),
        signal: exec.signal,
      });
      if (r.killed) throw new Error(`${name} was aborted or timed out`);
      let parsed = null;
      try {
        parsed = JSON.parse(r.stdout);
      } catch {
        parsed = null;
      }
      if (parsed && typeof parsed.error === "string") {
        throw new Error(parsed.error);
      }
      if (parsed && typeof parsed.text === "string") {
        const meta = [`session_id: ${parsed.session_id}`, `parent_message_id: ${parsed.parent_message_id}`]
          .filter((s) => !s.endsWith(": null") && !s.endsWith(": undefined"))
          .join(", ");
        return withVersionWarning(meta ? `${parsed.text}\n\n[${meta}]` : parsed.text);
      }
      throw new Error(
        `${name} failed (exit ${r.code}): ${(r.stderr || r.stdout).trim() || "unknown error"}`
      );
    } finally {
      for (const m of materialized) if (m.cleanup) await m.cleanup();
    }
  }

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision",
      description:
        "Analyze one or more images with DeepSeek's web vision model (local path / base64 / data URI). Pass multiple images via `images` to have the model analyze them together in one call (like the DeepSeek website). Use for screenshots, photos, or documents with images. Supports multi-turn conversation via continue_conversation / session_id.",
      parameters: imageToolParams,
      output: imageToolOutput,
      timeoutMs: cfg("visionTimeoutMs"),
      async execute(args, exec) {
        const imageInputs = Array.isArray(args.images) && args.images.length > 0
          ? args.images
          : args.image
            ? [args.image]
            : [];
        return runImageAnalysis({
          subcommand: "vision",
          name: "deepseek_vision",
          imageInputs,
          args,
          exec,
        });
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: "deepseek_ocr",
      description:
        "Extract raw text from one or more images with DeepSeek's OCR pipeline (local path / base64 / data URI), equivalent to `visionary-cli ocr`. Use for the text content of a screenshot or document — document/PDF screenshots, code, signs, tables (not interpretation). Text is extracted verbatim by default. Supports multi-turn conversation via continue_conversation / session_id.",
      parameters: imageToolParams,
      output: imageToolOutput,
      timeoutMs: cfg("visionTimeoutMs"),
      async execute(args, exec) {
        const imageInputs = Array.isArray(args.images) && args.images.length > 0
          ? args.images
          : args.image
            ? [args.image]
            : [];
        return runImageAnalysis({
          subcommand: "ocr",
          name: "deepseek_ocr",
          imageInputs,
          args,
          exec,
        });
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision_status",
      description:
        "Check DeepSeek Vision login state (authenticated, token validity). Run before deepseek_vision if unsure whether the user is logged in.",
      parameters: {},
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: cfg("statusTimeoutMs"),
      async execute(_args, exec) {
        const bin = requireBinary();
        const r = await runCli(bin, ["status", "--json"], {
          timeoutMs: cfg("statusTimeoutMs"),
          signal: exec.signal,
        });
        if (r.killed) throw new Error("deepseek_vision_status was aborted or timed out");
        // `status --json` prints a complete JSON document even when unauthenticated
        // and exits non-zero — the JSON is authoritative, not the exit code.
        let parsed = null;
        try {
          parsed = JSON.parse(r.stdout);
        } catch {
          parsed = null;
        }
        if (parsed) {
          return withVersionWarning(formatStatusResult(parsed));
        }
        throw new Error(
          `status failed (exit ${r.code}): ${(r.stderr || r.stdout).trim() || "unknown error"}`
        );
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision_login",
      description:
        "Browser auto-login to chat.deepseek.com (opens a browser window and waits for the user to log in). Blocks until login completes or the timeout elapses; the browser stays open on timeout so the user can finish and retry.",
      parameters: {},
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: loginSeconds * 1000,
      async execute(_args, exec) {
        const bin = requireBinary();
        const r = await runCli(bin, ["login"], {
          timeoutMs: loginSeconds * 1000,
          signal: exec.signal,
        });
        if (r.killed) {
          throw new Error(
            `deepseek_vision_login timed out after ${loginSeconds}s — the browser stays open; finish logging in and retry, or set DEEPSEEK_LOGIN_TIMEOUT for a longer wait.`
          );
        }
        if (r.code !== 0) {
          throw new Error(`login failed (exit ${r.code}): ${(r.stderr || r.stdout).trim()}`);
        }
        return withVersionWarning(r.stdout.trim() || "Logged in.");
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision_logout",
      description:
        "Clear the saved single-account credentials. When the account pool is enabled this does NOT remove pooled accounts and does NOT stop rotation — remove a specific account with deepseek_vision_accounts (action=remove).",
      parameters: {},
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: cfg("statusTimeoutMs"),
      async execute(_args, exec) {
        const bin = requireBinary();
        const r = await runCli(bin, ["logout"], {
          timeoutMs: cfg("statusTimeoutMs"),
          signal: exec.signal,
        });
        if (r.killed) throw new Error("deepseek_vision_logout was aborted or timed out");
        if (r.code !== 0) {
          throw new Error(`logout failed (exit ${r.code}): ${(r.stderr || r.stdout).trim()}`);
        }
        return withVersionWarning(r.stdout.trim() || "Logged out.");
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision_accounts",
      description:
        "Manage the DeepSeek account pool: action=list (show accounts), action=refresh (re-probe every account's health), action=remove (drop an account by id), action=pin (pin/unpin an account by id).",
      parameters: accountsToolParams,
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: accountsTimeout,
      async execute(args, exec) {
        const bin = requireBinary();
        const cliArgs = buildAccountsCliArgs(args.action, args.account_id, args.pinned);
        const r = await runCli(bin, cliArgs, {
          timeoutMs: accountsTimeout,
          signal: exec.signal,
        });
        if (r.killed) {
          throw new Error(
            "deepseek_vision_accounts was aborted or timed out before the CLI finished"
          );
        }
        // JSON 输出是权威结果（SHALL NOT 仅凭退出码判定成败）：CLI 失败时也会
        // 打印结构化 JSON，退出码只说明「有东西失败了」。
        let parsed = null;
        try {
          parsed = JSON.parse(r.stdout);
        } catch {
          parsed = null;
        }
        if (!parsed || typeof parsed !== "object") {
          throw new Error(
            `deepseek_vision_accounts failed (exit ${r.code}): ${(r.stderr || r.stdout).trim() || "unknown error"}`
          );
        }
        if (typeof parsed.error === "string") throw new Error(parsed.error);
        return withVersionWarning(formatAccountsResult(args.action, parsed));
      },
    })
  );
}

export {
  name,
  inject,
  Config,
  apply,
  SETTINGS_NAMESPACE,
  buildImageCliArgs,
  resolveBinaryPath,
  resolveFromNpmShim,
  COMPAT_MINOR,
};

// Internal reuse: the image-bridge plugin runs in the same package but is a
// separate plugin row; it needs the same binary resolution + subprocess
// plumbing to drive deterministic-mode analysis. Export the two pieces it
// reuses (not the public plugin surface).
export { runCli, binaryMissingHelp };

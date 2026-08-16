// DeepSeek Visionary — DeepSeek Harness native plugin.
//
// Registers `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login`
// / `deepseek_vision_logout` as native tools on `ctx.tools`. Every tool spawns the
// `visionary-server` binary (CLI; `--json` atomic output where the subcommand
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
import { statSync } from "node:fs";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

const name = "visionary-vision";
const inject = ["tools", "systemPrompt"];

// Keep in lockstep with the Rust binary's minor version: tools rely on the
// CLI's `--json` output shape. Bump when the binary's contract changes.
const COMPAT_MINOR = "0.6";

const Config = z.object({
  binaryPath: z
    .string()
    .default("")
    .description(
      "Absolute path to the visionary-server binary. Empty resolves via DEEPSEEK_VISIONARY_BIN, then PATH."
    ),
  loginTimeoutSeconds: z
    .number()
    .default(600)
    .description(
      "Login wait timeout in seconds. DEEPSEEK_LOGIN_TIMEOUT env overrides when set (matching the CLI's own login deadline); schema default 600."
    ),
  visionTimeoutMs: z
    .number()
    .default(300000)
    .description("Per deepseek_vision call timeout in ms."),
  statusTimeoutMs: z
    .number()
    .default(60000)
    .description("Per deepseek_vision_status / deepseek_vision_logout timeout in ms."),
});

// --- binary resolution -------------------------------------------------------

function resolveBinaryPath(config) {
  if (config.binaryPath) return config.binaryPath;
  const fromEnv = process.env.DEEPSEEK_VISIONARY_BIN;
  if (fromEnv) return fromEnv;
  const exe = process.platform === "win32" ? "visionary-server.exe" : "visionary-server";
  for (const dir of (process.env.PATH || "").split(path.delimiter).filter(Boolean)) {
    const candidate = path.join(dir, exe);
    try {
      if (statSync(candidate).isFile()) return candidate;
    } catch {
      // keep looking
    }
  }
  return null;
}

const binaryMissingHelp = () =>
  [
    "visionary-server binary not found. Install it and retry:",
    "  - One-liner: curl -LsSf https://github.com/xlight/deepseek-visionary/releases/latest/download/visionary-server-installer.sh | sh",
    "  - Homebrew: brew install <tap>/visionary-server",
    "  - npm: npm install -g @xlight-oss/visionary-server",
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

// --- version probe (apply-time, fire-and-forget) -----------------------------

function parseMinor(v) {
  const m = /(\d+)\.(\d+)/.exec(v || "");
  return m ? `${m[1]}.${m[2]}` : null;
}

// --- tools -------------------------------------------------------------------

function apply(ctx, config) {
  const binary = resolveBinaryPath(config);
  const loginSeconds = (() => {
    const raw = Number(process.env.DEEPSEEK_LOGIN_TIMEOUT);
    if (Number.isFinite(raw) && raw > 0) return raw;
    return config.loginTimeoutSeconds > 0 ? config.loginTimeoutSeconds : 600;
  })();

  let versionInfo = { known: false, compatible: true, version: "" };
  if (binary) {
    runCli(binary, ["--version"], { timeoutMs: 5000 })
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
    return `${text}\n\n[warning] visionary-server version "${versionInfo.version}" does not match the plugin's compatible version ${COMPAT_MINOR}.x — upgrade the binary or the plugin.`;
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
        "- `deepseek_vision_status` — check login state",
        "- `deepseek_vision_login` — browser auto-login",
        "- `deepseek_vision_logout` — clear saved credentials",
        "",
        "Prefer these native tools over invoking `visionary-server` through the shell: native tools run in the host process, so session continuation and login are not restricted by the bash sandbox.",
        "The `image`/`images` paths passed to `deepseek_vision` are read and uploaded to the DeepSeek service — only pass paths the user intends to share.",
      ].join("\n"),
    });
  }

  const requireBinary = () => {
    if (!binary) throw new Error(binaryMissingHelp());
    return binary;
  };

  ctx.tools.register(
    defineTool({
      name: "deepseek_vision",
      description:
        "Analyze one or more images with DeepSeek's web vision model (local path / base64 / data URI). Pass multiple images via `images` to have the model analyze them together in one call (like the DeepSeek website). Use for screenshots, photos, or documents with images. Supports multi-turn conversation via continue_conversation / session_id.",
      parameters: {
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
      },
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: config.visionTimeoutMs,
      async execute(args, exec) {
        const bin = requireBinary();
        const imageInputs = Array.isArray(args.images) && args.images.length > 0
          ? args.images
          : args.image
            ? [args.image]
            : [];
        if (imageInputs.length === 0) {
          throw new Error("deepseek_vision: at least one image is required (`images` or `image`)");
        }
        const materialized = [];
        try {
          for (const image of imageInputs) {
            materialized.push(await materializeImage(image));
          }
          const cliArgs = ["vision", ...materialized.map((m) => m.arg), "--json"];
          if (args.prompt) cliArgs.push(`--prompt=${args.prompt}`);
          if (args.thinking) cliArgs.push("--thinking");
          if (args.session_id) cliArgs.push(`--session-id=${args.session_id}`);
          else if (args.continue_conversation) cliArgs.push("--continue-conversation");

          const r = await runCli(bin, cliArgs, {
            timeoutMs: config.visionTimeoutMs,
            signal: exec.signal,
          });
          if (r.killed) throw new Error("deepseek_vision was aborted or timed out");
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
            `vision failed (exit ${r.code}): ${(r.stderr || r.stdout).trim() || "unknown error"}`
          );
        } finally {
          for (const m of materialized) if (m.cleanup) await m.cleanup();
        }
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
      timeoutMs: config.statusTimeoutMs,
      async execute(_args, exec) {
        const bin = requireBinary();
        const r = await runCli(bin, ["status", "--json"], {
          timeoutMs: config.statusTimeoutMs,
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
          const state = parsed.token_valid
            ? "authenticated, token valid"
            : parsed.authenticated
              ? "authenticated (live probe pending)"
              : "not logged in";
          const hint = parsed.token_valid
            ? ""
            : "\nRun `deepseek_vision_login` to auto-login, or set DEEPSEEK_USER_TOKEN.";
          return withVersionWarning(`DeepSeek Vision status: ${state}${hint}`);
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
      description: "Clear saved DeepSeek Vision credentials.",
      parameters: {},
      output: {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
      },
      timeoutMs: config.statusTimeoutMs,
      async execute(_args, exec) {
        const bin = requireBinary();
        const r = await runCli(bin, ["logout"], {
          timeoutMs: config.statusTimeoutMs,
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
}

export { name, inject, Config, apply };

// VisionaryDashboard — host Service exposing aggregated runtime status and
// diagnostic actions to the browser via Typert Remote.
//
// Registered as a `visionary-dashboard` entry in cordis.patch.yml. The browser
// half (lib/client.js) calls `ctx.remote.visionaryDashboard.getStatus()` etc.
// through the Typert Gateway, which proxies these methods over the session wire.
//
// The class extends `TypertRemoteService` (which extends cordis `Service`) and
// marks its public methods with the `@Remote` decorator. Because this package
// ships hand-written ESM without a build step, we apply the decorators using
// the same `__esDecorate` helper pattern that tsdown emits for compiled
// decorator syntax (see @deepseek-ai/dsh-experimental-api-speech-to-text for
// the reference output). This keeps the runtime behavior identical to a
// `@Remote` annotated class without requiring a transpiler.

import { Remote, RemoteError, TypertRemoteService } from "@deepseek-ai/dsh-typert-protocol";
import {
  resolveBinaryPath,
  runCli,
  spawnDetached,
  volGet,
  binaryMissingHelp,
  COMPAT_MINOR,
} from "./index.mjs";
import { countPastedDir, cleanupPastedDir } from "./image-bridge/persistence.mjs";

// --- decorator application helpers (tsdown-emitted pattern) -----------------

function __runInitializers(thisArg, initializers, value) {
  const useValue = arguments.length > 2;
  for (let i = 0; i < initializers.length; i++) {
    value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg);
  }
  return useValue ? value : undefined;
}

function __esDecorate(ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {
  function accept(f) {
    if (f !== undefined && typeof f !== "function") throw new TypeError("Function expected");
    return f;
  }
  const kind = contextIn.kind;
  const key = kind === "getter" ? "get" : kind === "setter" ? "set" : "value";
  const target = !descriptorIn && ctor ? (contextIn.static ? ctor : ctor.prototype) : null;
  let descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {});
  let done = false;
  for (let i = decorators.length - 1; i >= 0; i--) {
    const context = {};
    for (const p in contextIn) context[p] = p === "access" ? {} : contextIn[p];
    for (const p in contextIn.access) context.access[p] = contextIn.access[p];
    context.addInitializer = function (f) {
      if (done) throw new TypeError("Cannot add initializers after decoration has completed");
      extraInitializers.push(accept(f || null));
    };
    const result = decorators[i](
      kind === "accessor" ? { get: descriptor.get, set: descriptor.set } : descriptor[key],
      context,
    );
    if (kind === "accessor") {
      if (result === undefined) continue;
      if (result === null || typeof result !== "object")
        throw new TypeError("Object expected");
      if (result.get !== undefined) descriptor.get = result.get;
      if (result.set !== undefined) descriptor.set = result.set;
      if (result.init !== undefined) initializers.push(accept(result.init));
    } else if (result !== undefined) {
      descriptor[key] = result;
    }
  }
  if (target) Object.defineProperty(target, contextIn.name, descriptor);
  done = true;
}

// --- version minor parser (inlined from index.mjs, which does not export it) --

function parseMinor(v) {
  const m = /(\d+)\.(\d+)/.exec(v || "");
  return m ? `${m[1]}.${m[2]}` : null;
}

// --- VisionaryDashboard Service ----------------------------------------------

let VisionaryDashboard = (() => {
  const _classSuper = TypertRemoteService;
  const _instanceExtraInitializers = [];
  let _getStatus_decorators;
  let _reprobeBinary_decorators;
  let _relogin_decorators;
  let _cleanPasted_decorators;
  let _selfCheck_decorators;
  let _getAccounts_decorators;
  let _refreshAccounts_decorators;
  let _addAccount_decorators;
  let _removeAccount_decorators;
  let _pinAccount_decorators;
  return class VisionaryDashboard extends _classSuper {
    static {
      const _metadata =
        typeof Symbol === "function" && Symbol.metadata
          ? Object.create(_classSuper[Symbol.metadata] ?? null)
          : undefined;
      _getStatus_decorators = [Remote];
      _reprobeBinary_decorators = [Remote];
      _relogin_decorators = [Remote];
      _cleanPasted_decorators = [Remote];
      _selfCheck_decorators = [Remote];
      _getAccounts_decorators = [Remote];
      _refreshAccounts_decorators = [Remote];
      _addAccount_decorators = [Remote];
      _removeAccount_decorators = [Remote];
      _pinAccount_decorators = [Remote];
      for (const [name, decorators] of [
        ["getStatus", _getStatus_decorators],
        ["reprobeBinary", _reprobeBinary_decorators],
        ["relogin", _relogin_decorators],
        ["cleanPasted", _cleanPasted_decorators],
        ["selfCheck", _selfCheck_decorators],
        ["getAccounts", _getAccounts_decorators],
        ["refreshAccounts", _refreshAccounts_decorators],
        ["addAccount", _addAccount_decorators],
        ["removeAccount", _removeAccount_decorators],
        ["pinAccount", _pinAccount_decorators],
      ]) {
        __esDecorate(
          this,
          null,
          decorators,
          {
            kind: "method",
            name,
            static: false,
            private: false,
            access: { has: (obj) => name in obj, get: (obj) => obj[name] },
            metadata: _metadata,
          },
          null,
          _instanceExtraInitializers,
        );
      }
      if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });
    }

    /**
     * @param ctx - owning Cordis Context.
     * @param config - { visionEntryId, bridgeEntryId } pointing at the two
     *   sibling plugin entries whose config this dashboard aggregates.
     */
    constructor(ctx, config) {
      super(ctx, "visionaryDashboard");
      this.config = config;
      __runInitializers(this, _instanceExtraInitializers);
    }

    // --- config accessors ----------------------------------------------------

    get #visionConfig() {
      const svc = this.ctx.get?.(this.config.visionEntryId);
      return svc?.config ?? {};
    }

    get #bridgeConfig() {
      const svc = this.ctx.get?.(this.config.bridgeEntryId);
      return svc?.config ?? {};
    }

    #visionBinaryPath() {
      return volGet(this.#visionConfig.binaryPath ?? "");
    }

    #visionStatusTimeout() {
      const v = volGet(this.#visionConfig.statusTimeoutMs);
      return Number.isFinite(v) && v > 0 ? v : 60000;
    }

    #bridgePastedDir() {
      return volGet(this.#bridgeConfig.pastedDir ?? "");
    }

    // --- aggregated status ---------------------------------------------------

    /**
     * Aggregated runtime status: binary readiness/version, login/session, and
     * bridge running state. All three run in parallel; each degrades to an
     * error shape on failure rather than rejecting the whole call.
     */
    async getStatus() {
      const [binary, login, bridge] = await Promise.all([
        this.#binaryStatus().catch((err) => ({ ready: false, error: msg(err) })),
        this.#loginStatus().catch((err) => ({ ok: false, error: msg(err) })),
        this.#bridgeStatus().catch((err) => ({ enabled: false, error: msg(err) })),
      ]);
      return { binary, login, bridge };
    }

    async #binaryStatus() {
      const bin = resolveBinaryPath({ binaryPath: this.#visionBinaryPath() });
      if (!bin) {
        return {
          ready: false,
          path: null,
          version: null,
          compatible: false,
          expectedMinor: COMPAT_MINOR,
          hint: binaryMissingHelp(),
        };
      }
      let version = null;
      try {
        const r = await runCli(bin, ["--version"], { timeoutMs: 5000 });
        version = (r.stdout || r.stderr).trim() || null;
      } catch {
        version = null;
      }
      const minor = version ? parseMinor(version) : null;
      const compatible = minor === COMPAT_MINOR;
      return {
        ready: true,
        path: bin,
        version,
        compatible,
        expectedMinor: COMPAT_MINOR,
      };
    }

    async #loginStatus() {
      const bin = resolveBinaryPath({ binaryPath: this.#visionBinaryPath() });
      if (!bin) return { ok: false, error: "binary not found" };
      const r = await runCli(bin, ["status", "--json"], {
        timeoutMs: this.#visionStatusTimeout(),
      });
      if (r.code !== 0) {
        return { ok: false, error: (r.stderr || `exit ${r.code}`).trim() };
      }
      try {
        const parsed = JSON.parse(r.stdout);
        return { ok: true, ...parsed };
      } catch {
        return { ok: false, error: "status output was not valid JSON" };
      }
    }

    async #bridgeStatus() {
      const cfg = this.#bridgeConfig;
      const enabled = volGet(cfg.enabled);
      if (!enabled) {
        return { enabled: false, scope: null, mode: null, pastedCount: null };
      }
      const scope = volGet(cfg.scope);
      const mode = volGet(cfg.mode);
      const pastedDir = this.#bridgePastedDir();
      const pastedCount = pastedDir ? await countPastedDir(pastedDir) : 0;
      return { enabled: true, scope, mode, pastedCount };
    }

    // --- diagnostic actions --------------------------------------------------

    /** Re-run resolveBinaryPath and return the fresh binary status. */
    async reprobeBinary() {
      return this.#binaryStatus().catch((err) => ({
        ready: false,
        error: msg(err),
      }));
    }

    /**
     * Trigger `visionary-cli login` (async, non-blocking). Returns when the
     * login command exits; the browser may poll getStatus() for the result.
     */
    async relogin() {
      const bin = resolveBinaryPath({ binaryPath: this.#visionBinaryPath() });
      if (!bin) throw new RemoteError("visionary/binary-missing", binaryMissingHelp());
      const r = await runCli(bin, ["login"], {
        timeoutMs: (this.#visionStatusTimeout() || 60000) * 10,
      });
      if (r.code !== 0) {
        throw new RemoteError(
          "visionary/login-failed",
          (r.stderr || `exit ${r.code}`).trim(),
        );
      }
      return { ok: true };
    }

    /**
     * Clean every pasted copy under the bridge directory. Returns the number of
     * files removed. Reuses the same scanning semantics as the bridge's own
     * cleanPasted trigger.
     */
    async cleanPasted() {
      const dir = this.#bridgePastedDir();
      if (!dir) return { removed: 0 };
      const removed = await cleanupPastedDir(dir, { all: true }, this.ctx.logger);
      return { removed };
    }

    /**
     * Run a binary + login check and return the combined result. Used by the
     * "self-check" diagnostic button.
     */
    async selfCheck() {
      const [binary, login] = await Promise.all([
        this.#binaryStatus().catch((err) => ({ ready: false, error: msg(err) })),
        this.#loginStatus().catch((err) => ({ ok: false, error: msg(err) })),
      ]);
      return { binary, login };
    }

    // --- account pool ---------------------------------------------------------

    /** 账号池概况（纯读：`accounts list --json`）。 */
    async getAccounts() {
      return this.#accountsJson(["accounts", "list", "--json"]);
    }

    /** 刷新账号健康（并发只读探针：`accounts status --json`）。 */
    async refreshAccounts() {
      return this.#accountsJson(["accounts", "status", "--json"]);
    }

    /**
     * 添加账号：**非阻塞**的「启动」语义——立即返回 { started: true }，
     * 客户端沿用既有的 status 轮询呈现「等待登录完成」（不另造轮询机制）。
     */
    async addAccount() {
      const bin = resolveBinaryPath({ binaryPath: this.#visionBinaryPath() });
      if (!bin) throw new RemoteError("visionary/binary-missing", binaryMissingHelp());
      spawnDetached(bin, ["accounts", "add"], {
        env: { DEEPSEEK_LOGIN_TIMEOUT: String(this.#loginTimeoutSeconds()) },
      });
      return { started: true };
    }

    /** 移除账号（`accounts remove --json <id>`）。 */
    async removeAccount(id) {
      const target = String(id ?? "").trim();
      if (!target) {
        throw new RemoteError("visionary/accounts-invalid", "account id is required");
      }
      return this.#accountsJson(["accounts", "remove", "--json", target]);
    }

    /** 固定 / 取消固定账号（`id` 为 null 表示取消固定）。 */
    async pinAccount(id) {
      const target = id === null || id === undefined ? null : String(id).trim();
      const args = target
        ? ["accounts", "use", "--json", target]
        : ["accounts", "use", "--json", "--clear"];
      return this.#accountsJson(args);
    }

    async #accountsJson(args) {
      const bin = resolveBinaryPath({ binaryPath: this.#visionBinaryPath() });
      if (!bin) throw new RemoteError("visionary/binary-missing", binaryMissingHelp());
      const timeoutMs = this.#accountsTimeout();
      const r = await runCli(bin, args, { timeoutMs });
      if (r.killed) {
        throw new RemoteError(
          "visionary/accounts-timeout",
          `accounts command timed out after ${timeoutMs}ms`,
        );
      }
      let parsed = null;
      try {
        parsed = JSON.parse(r.stdout);
      } catch {
        parsed = null;
      }
      if (!parsed || typeof parsed !== "object") {
        throw new RemoteError(
          "visionary/accounts-failed",
          (r.stderr || r.stdout || `exit ${r.code}`).trim() || `exit ${r.code}`,
        );
      }
      if (typeof parsed.error === "string") {
        throw new RemoteError("visionary/accounts-failed", parsed.error);
      }
      return parsed;
    }

    // 账号池批量探测的客户端超时：必须大于 CLI 侧预算（见 index.mjs 同名逻辑）。
    #accountsTimeout() {
      const raw = Number(process.env.DEEPSEEK_ACCOUNTS_TIMEOUT);
      const cliBudgetMs = Number.isFinite(raw) && raw > 0 ? raw * 1000 : 30000;
      const configured = Number(volGet(this.#visionConfig.accountsTimeoutMs));
      const base = Number.isFinite(configured) && configured > 0 ? configured : 30000;
      return Math.max(base, cliBudgetMs) + 15000;
    }

    #loginTimeoutSeconds() {
      const raw = Number(process.env.DEEPSEEK_LOGIN_TIMEOUT);
      if (Number.isFinite(raw) && raw > 0) return raw;
      const configured = Number(volGet(this.#visionConfig.loginTimeoutSeconds));
      return Number.isFinite(configured) && configured > 0 ? configured : 600;
    }

  };
})();

function msg(err) {
  return err?.message ?? String(err);
}

export { VisionaryDashboard as default };

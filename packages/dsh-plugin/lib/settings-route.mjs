// Fenced settings HTTP routes for the visionary settings-card panel.
//
// The DSH settings RPC domain (dsh-host-apiproxy) only serves an explicit
// allowlist of namespaces to Web configuration clients — a third-party
// plugin's namespace never appears in `connection.api.settings.describe()`
// no matter how correctly it is registered in-process. The established
// pattern for a third-party settings page (see dsh-better-sidebar,
// dsh-at-file) is a private, fenced HTTP/Remote route that calls
// `ctx.settings` in-process instead. This module mounts that route for the
// settings-card client half (`../settings-card/client.js`).
//
// The route is namespace-aware: every method accepts an optional `ns` body
// field naming a registered visionary namespace to read/write. The caller
// (settings-card host) passes the namespace list; the default when `ns` is
// absent is the first entry (image-bridge) for backward compatibility with
// older deployed clients. Unknown namespaces are rejected (400), so the route
// can never mutate a namespace it does not own.
//
// The route lives on the settings-card host row (not on either feature
// plugin) so the panel keeps working when a feature plugin row is disabled:
// each plugin registers its own settings namespace via installSettingsSection
// (loading is independent of the panel UI).
//
// webServer exists only in Web profiles. It is NOT a hard dependency of the
// settings-card row (headless compositions would never load the panel), so
// the route rides the same optional-service pattern installSettingsSection
// uses for `settings`: inject-wait for the service, register inside the
// nested fiber, dispose with it.

import { SettingsConflictError } from "@deepseek-ai/dsh-settings";
import { isTrustedApiRequest } from "./image-bridge/trust-fence.mjs";

const readJsonBody = (req) =>
  new Promise((resolve, reject) => {
    let raw = "";
    req.on("data", (chunk) => {
      raw += chunk;
      if (raw.length > 1_000_000) {
        reject(new Error("request body too large"));
        req.destroy();
      }
    });
    req.on("end", () => {
      if (raw === "") {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(raw));
      } catch (err) {
        reject(new Error(`invalid JSON body: ${err?.message ?? err}`));
      }
    });
    req.on("error", reject);
  });

const writeJson = (res, status, body) => {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
};

/** Namespace resolution state, bound per mount (default = first namespace). */
function makeNamespaceResolver(nsList) {
  return function resolveNamespace(body) {
    const ns = body?.ns;
    if (ns === undefined || ns === null || ns === "") return nsList[0];
    return nsList.includes(ns) ? ns : null;
  };
}

/** Redacted-free view: these namespaces carry no secrets, so the settings
 * document itself is returned verbatim (no redactSecrets pass needed).
 * `base`/`user` ride along so the client can mark which fields the user
 * explicitly overrode (the settings-card "overridden" badge). */
function currentView(settings, ns) {
  if (settings === undefined) {
    return { value: undefined, base: undefined, user: undefined, revision: undefined, writable: false };
  }
  const descriptor = settings
    .describe()
    .find((candidate) => candidate.ns === ns);
  return descriptor === undefined
    ? { value: undefined, base: undefined, user: undefined, revision: undefined, writable: settings.writable }
    : {
        value: descriptor.value,
        base: descriptor.base,
        user: descriptor.user,
        revision: descriptor.revision,
        writable: settings.writable,
      };
}

/**
 * Mount the `/visionary/api` settings route on a webServer-equipped fiber.
 * @param webCtx - nested fiber that already resolved `webServer`.
 * @param namespaces - the settings namespaces this plugin family may serve,
 *  in the order shown to clients. The first entry (image-bridge) is the
 *  default when a client sends no `ns` (backward compatibility).
 */
export function mountVisionaryApi(webCtx, namespaces) {
  const webServer = webCtx.webServer;
  const resolveNamespace = makeNamespaceResolver(namespaces);
  const trustedHosts = () => {
    const webRuntime = webCtx.get("webRuntime");
    return Array.isArray(webRuntime?.trustedHosts) ? webRuntime.trustedHosts : [];
  };
  const fence = (req) => isTrustedApiRequest(req, trustedHosts());

  webCtx.effect(
    () =>
      webServer.register({
        kind: "prefix",
        path: "/visionary/api",
        handler: async (req, res) => {
          if (!fence(req)) {
            writeJson(res, 403, { ok: false, error: { code: "forbidden", message: "forbidden" } });
            return;
          }
          if (req.method !== "POST") {
            writeJson(res, 405, { ok: false, error: { code: "method-error", message: "method not allowed" } });
            return;
          }
          const pathname = new URL(req.url ?? "/", "http://dsh.internal").pathname;
          const method = pathname.startsWith("/visionary/api/")
            ? pathname.slice("/visionary/api/".length)
            : undefined;
          try {
            const settings = webCtx.get("settings");
            if (method === "settings.get") {
              const body = await readJsonBody(req);
              const ns = resolveNamespace(body);
              if (ns === null) {
                writeJson(res, 400, { ok: false, error: { code: "bad-request", message: `unknown namespace "${body?.ns}"` } });
                return;
              }
              writeJson(res, 200, { ok: true, value: currentView(settings, ns) });
              return;
            }
            if (settings === undefined) {
              writeJson(res, 503, {
                ok: false,
                error: { code: "settings-rejected", message: "the settings service is not mounted in this deployment" },
              });
              return;
            }
            if (method === "settings.update") {
              const body = await readJsonBody(req);
              const ns = resolveNamespace(body);
              if (ns === null) {
                writeJson(res, 400, { ok: false, error: { code: "bad-request", message: `unknown namespace "${body?.ns}"` } });
                return;
              }
              const patch = body?.patch;
              if (patch === null || typeof patch !== "object" || Array.isArray(patch)) {
                writeJson(res, 400, { ok: false, error: { code: "bad-request", message: "patch must be a plain object" } });
                return;
              }
              const expectedRevision = typeof body?.expectedRevision === "number" ? body.expectedRevision : undefined;
              try {
                await settings.update(ns, patch, expectedRevision);
              } catch (err) {
                if (err instanceof SettingsConflictError) {
                  writeJson(res, 409, { ok: false, error: { code: "settings-conflict", message: err.message } });
                  return;
                }
                writeJson(res, 400, {
                  ok: false,
                  error: { code: "settings-rejected", message: err instanceof Error ? err.message : String(err) },
                });
                return;
              }
              writeJson(res, 200, { ok: true, value: currentView(settings, ns) });
              return;
            }
            if (method === "settings.mutate") {
              const body = await readJsonBody(req);
              const ns = resolveNamespace(body);
              if (ns === null) {
                writeJson(res, 400, { ok: false, error: { code: "bad-request", message: `unknown namespace "${body?.ns}"` } });
                return;
              }
              const ops = body?.ops;
              if (!Array.isArray(ops)) {
                writeJson(res, 400, { ok: false, error: { code: "bad-request", message: "ops must be an array" } });
                return;
              }
              const expectedRevision = typeof body?.expectedRevision === "number" ? body.expectedRevision : undefined;
              try {
                await settings.mutate(ns, ops, expectedRevision);
              } catch (err) {
                if (err instanceof SettingsConflictError) {
                  writeJson(res, 409, { ok: false, error: { code: "settings-conflict", message: err.message } });
                  return;
                }
                writeJson(res, 400, {
                  ok: false,
                  error: { code: "settings-rejected", message: err instanceof Error ? err.message : String(err) },
                });
                return;
              }
              writeJson(res, 200, { ok: true, value: currentView(settings, ns) });
              return;
            }
            writeJson(res, 404, { ok: false, error: { code: "not-found", message: `unknown visionary API method "${method}"` } });
          } catch (err) {
            writeJson(res, 400, {
              ok: false,
              error: { code: "bad-request", message: err instanceof Error ? err.message : String(err) },
            });
          }
        },
      }),
    "visionary-settings-card: /visionary/api settings route",
  );
}
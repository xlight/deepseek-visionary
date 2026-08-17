// Dependency-free core factories for the visionary image bridge.
//
// Each factory takes its dependencies explicitly so the unit tests run without
// a cordis context or a node_modules install; `lib/index.mjs` wires them to
// the real host services.

import {
  messagesHaveImage,
  renderAnalysis,
  renderGuide,
  rewriteMessages,
} from "./rewrite.mjs";

/**
 * Route matching for the bridge. An empty `routes` list means ALL routes are
 * bridged (the plugin is global and cannot see "the current session's route");
 * an explicit list admits only the listed (provider, model) pairs, where a
 * `model` of `"*"` / `""` / omitted matches every model of that provider.
 */
export function matchesRoute(provider, model, routes) {
  if (!Array.isArray(routes) || routes.length === 0) return true;
  return routes.some((route) => {
    if (!route || route.provider !== provider) return false;
    const wanted = route.model;
    if (wanted === undefined || wanted === "*" || wanted === "") return true;
    return wanted === model;
  });
}

/**
 * ModelInfoPatcher (change task 2.1): wrap `llm.resolveModelInfo` so enabled
 * bridge routes additionally report the `image` input modality — the host
 * apiproxy admission gate reads `inputModalities` and rejects image prompts
 * with MODEL_DOES_NOT_SUPPORT_IMAGES otherwise. Everything else passes through
 * untouched.
 *
 * The ORIGINAL method reference is kept separate from the patch so capability
 * sensing (`nativeImageCapable`) and the imageRouting consultation are never
 * poisoned by it — the patch is strictly an admission-release lever, and
 * without this separation every bridge route would read as "natively image
 * capable" and the rewrite would never run (images would hit the pi-ai second
 * gate and fail with UNSUPPORTED_CONTENT).
 *
 * Lifecycle: `install()` registers the patch and returns a disposer restoring
 * the original (re-assign, guarded so it never clobbers a later patch by
 * another plugin). The plugin registers the disposer through `ctx.effect`, so
 * unload/HMR reload always restores the original — otherwise a leftover patch
 * would be captured as the "original" by the next apply and poison capability
 * sensing forever.
 */
export function makeModelInfoPatch({ llm, isEnabled, routeMatch = matchesRoute }) {
  const original = llm.resolveModelInfo.bind(llm);
  const patched = async (provider, model, signal) => {
    const info = await original(provider, model, signal);
    if (!isEnabled() || !routeMatch(provider, model)) return info;
    if (!Array.isArray(info?.inputModalities) || info.inputModalities.includes("image")) {
      return info;
    }
    return { ...info, inputModalities: [...info.inputModalities, "image"] };
  };
  return {
    /** The unpatched method, for capability sensing and consultations. */
    original,
    /** Install the patch; returns the disposer that restores the original. */
    install() {
      llm.resolveModelInfo = patched;
      return () => {
        if (llm.resolveModelInfo === patched) llm.resolveModelInfo = original;
      };
    },
  };
}

/**
 * Native image capability of one route (change task 3.3), judged through the
 * UNPATCHED resolveModelInfo — never the patched method, which always reports
 * `image` for bridge routes. VL models return true and are left untouched
 * (they view images natively); text-only models return false and get rewritten.
 */
export async function nativeImageCapable(originalResolveModelInfo, provider, model, signal) {
  try {
    const info = await originalResolveModelInfo(provider, model, signal);
    return Array.isArray(info?.inputModalities) && info.inputModalities.includes("image");
  } catch {
    // A capability lookup failure here is unreachable through the normal flow
    // (admission already succeeded through the patched method, which delegates
    // to the original). Treat unknown as not-natively-capable so the bridge
    // rewrite still produces a text-only request rather than failing the call.
    return false;
  }
}

/**
 * StreamRewriter listener (change tasks 3.2 / 3.4 / 3.5): the `llm/stream`
 * waterfall listener that vetoes and re-enters when the request is on a bridge
 * route, carries images, and the route is not natively image-capable.
 *
 * - `options` is deep-frozen and the waterfall's `next()` closure binds the
 *   original options, so mutation is impossible and args rewriting is
 *   ineffective: the listener returns an async generator that rewrites the
 *   messages and re-enters `llm.stream({ ...options, messages: rewritten })`
 *   (depth is always 2 — the rewrite is idempotent, pass-2 has no images).
 * - Capability sensing runs per request, so switching to a VL model mid-session
 *   automatically restores native image viewing for historical images.
 * - The hard recursion guard (task 3.4) tracks rewritten batches by message
 *   array identity in a WeakSet: a defective rewrite that leaves images behind
 *   hits the guard on pass-2 and falls through to `next()` (the adapter's own
 *   error handling) instead of recursing forever.
 * - Storage failures degrade per image to a text placeholder (task 3.5), so
 *   the rewritten request stays image-free, the conversation continues, and
 *   nothing is thrown or swallowed; cancellation is never masked.
 */
export function makeStreamListener({
  llm,                       // ctx.llm — reentry target for the rewritten request
  originalResolveModelInfo,  // unpatched resolveModelInfo (capability sensing)
  getRuntime,                // () => { enabled, routes, promptTemplate, scope, mode }
  persistence,               // ImagePersistence (persist(block.attachment, signal) -> path)
  rewrittenBatches,          // WeakSet — hard recursion guard (task 3.4)
  logger,                    // ctx.logger (optional)
  routeMatch = matchesRoute,
  placeholder = "用户粘贴的图片处理失败，无法分析。",
  analyzeImage = null,       // deterministic-mode hook: (filePath, signal) -> analysis text
}) {
  return (options, next) => {
    if (!options || !options.messages || !options.provider) return next();
    const runtime = getRuntime();
    if (!runtime.enabled) return next();
    if (!routeMatch(options.provider, options.model, runtime.routes)) return next();
    if (!messagesHaveImage(options.messages)) return next();
    if (rewrittenBatches.has(options.messages)) return next(); // hard guard

    return (async function* () {
      const native = await nativeImageCapable(
        originalResolveModelInfo,
        options.provider,
        options.model,
        options.signal,
      );
      // scope：text-only（默认）VL 模型原生看图不干预；also-vl 时 VL 模型同样经桥接改写。
      const scope = runtime.scope === "also-vl" ? "also-vl" : "text-only";
      if (native && scope !== "also-vl") {
        // VL model: view images natively, never rewrite.
        yield* next();
        return;
      }
      const resolveGuide = async (block, signal) => {
        let filePath;
        try {
          filePath = await persistence.persist(block.attachment, signal);
        } catch (err) {
          if (signal?.aborted) throw err; // never mask cancellation
          logger?.warn?.(`[visionary-image-bridge] image persistence failed: ${err?.message ?? err}`);
          return placeholder;
        }
        // deterministic：桥接自行调用分析并注入带不可信标注的结果文本；失败降级占位。
        if (runtime.mode === "deterministic" && analyzeImage) {
          try {
            return renderAnalysis(await analyzeImage(filePath, signal));
          } catch (err) {
            if (signal?.aborted) throw err; // never mask cancellation
            logger?.warn?.(
              `[visionary-image-bridge] deterministic analysis failed: ${err?.message ?? err}`
            );
            return placeholder;
          }
        }
        return renderGuide(runtime.promptTemplate, filePath);
      };
      let rewritten;
      try {
        rewritten = await rewriteMessages(options.messages, resolveGuide, options.signal);
      } catch (err) {
        if (options.signal?.aborted) throw err;
        logger?.warn?.(`[visionary-image-bridge] rewrite failed: ${err?.message ?? err}`);
        yield* next(); // never swallow the request
        return;
      }
      if (rewritten === options.messages) {
        yield* next();
        return;
      }
      rewrittenBatches.add(rewritten);
      yield* llm.stream({ ...options, messages: rewritten });
    })();
  };
}

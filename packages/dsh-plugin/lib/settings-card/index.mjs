// Host half of the settings-card sub-entry.
// The client half (client.js) registers the settings sections in the browser;
// the host half mounts the fenced /visionary/api settings route that serves
// BOTH visionary namespaces (visionary-image-bridge + visionary-vision) to
// those sections. Kept on this row (not on either feature plugin) so the
// panel keeps working when a feature row is disabled — each plugin registers
// its own namespace via installSettingsSection independently.
import { SETTINGS_NAMESPACE as BRIDGE_NAMESPACE } from "../image-bridge/index.mjs";
import { SETTINGS_NAMESPACE as VISION_NAMESPACE } from "../index.mjs";
import { mountVisionaryApi } from "../settings-route.mjs";

export const name = "visionary-settings-card";

export function apply(ctx) {
  ctx.inject(["webServer"], (webCtx) => {
    mountVisionaryApi(webCtx, [BRIDGE_NAMESPACE, VISION_NAMESPACE]);
  });
}
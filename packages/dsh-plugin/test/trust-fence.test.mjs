// Unit tests for lib/image-bridge/trust-fence.mjs (the browser-trust fence
// gating the /visionary/api routes). Dependency-free — runs without
// node_modules.

import test from "node:test";
import assert from "node:assert/strict";
import { isTrustedApiRequest, isLoopbackHostname } from "../lib/image-bridge/trust-fence.mjs";

/**
 * Build a minimal request-shaped object. `host` and headers default to a
 * same-origin loopback request (the common GUI case).
 */
const req = ({ host = "127.0.0.1:3080", origin, secFetchSite } = {}) => ({
  headers: {
    host,
    ...(origin !== undefined ? { origin } : {}),
    ...(secFetchSite !== undefined ? { "sec-fetch-site": secFetchSite } : {}),
  },
});

// ------------------------------------------------------------------ loopback

test("loopback hostnames are recognized (localhost, 127.x, ::1)", () => {
  assert.equal(isLoopbackHostname("localhost"), true);
  assert.equal(isLoopbackHostname("[::1]"), true);
  assert.equal(isLoopbackHostname("127.0.0.1"), true);
  assert.equal(isLoopbackHostname("127.1.2.3"), true);
  assert.equal(isLoopbackHostname("example.com"), false);
  assert.equal(isLoopbackHostname("192.168.1.5"), false);
});

// ------------------------------------------------------------------ fence

test("accepts a same-origin loopback request with no browser markers", () => {
  assert.equal(isTrustedApiRequest(req(), []), true);
});

test("accepts the loopback origin exactly", () => {
  assert.equal(isTrustedApiRequest(req({ origin: "http://127.0.0.1:3080" }), []), true);
});

test("accepts localhost origin against a loopback host", () => {
  assert.equal(isTrustedApiRequest(req({ host: "localhost:3080", origin: "http://localhost:3080" }), []), true);
});

test("rejects a cross-site browser marker", () => {
  assert.equal(isTrustedApiRequest(req({ secFetchSite: "cross-site" }), []), false);
});

test("rejects a mismatched origin", () => {
  assert.equal(isTrustedApiRequest(req({ origin: "http://evil.example" }), []), false);
});

test("rejects an unparsable origin", () => {
  assert.equal(isTrustedApiRequest(req({ origin: "not a url" }), []), false);
});

test("rejects a missing Host header", () => {
  assert.equal(isTrustedApiRequest({ headers: {} }, []), false);
});

test("rejects a non-loopback, untrusted host even same-origin", () => {
  assert.equal(
    isTrustedApiRequest(req({ host: "192.168.1.10:3080", origin: "http://192.168.1.10:3080" }), []),
    false,
  );
});

test("accepts a LAN host listed in trustedHosts", () => {
  assert.equal(
    isTrustedApiRequest(
      req({ host: "192.168.1.10:3080", origin: "http://192.168.1.10:3080" }),
      ["192.168.1.10:3080"],
    ),
    true,
  );
});

test("trustedHosts entry without a port matches a default-port authority", () => {
  assert.equal(
    isTrustedApiRequest(
      req({ host: "dsh.example:80", origin: "http://dsh.example" }),
      ["dsh.example"],
    ),
    true,
  );
});
// Browser-trust fence for the visionary settings HTTP routes.
//
// Behavioral mirror of the /api gateway's fence in @deepseek-ai/dsh-client-connection
// (api-request-trust.ts, BSD-3-Clause). The DSH settings RPC domain only serves
// allowlisted namespaces to configuration clients, so a third-party plugin's
// namespace is unreachable through `connection.api.settings.*`. The
// settings-card host mounts its own fenced JSON routes instead (see
// lib/settings-route.mjs `visionary/api`, serving both the vision-tools and
// image-bridge namespaces); the fence below is a DNS-rebinding / cross-site
// defense, NOT authentication — the web browser is already same-origin with
// the host.
//
// Dependency-free (pure functions over node:http headers) so unit tests run
// without a node_modules install.

/** The request facts the fence reads (structural subset of IncomingMessage). */
function header(headers, name) {
  const value = headers[name];
  return typeof value === "string" ? value : undefined;
}

/** Normalized URL of a Host-header authority, or undefined when unparsable. */
function parseAuthority(authority) {
  try {
    return new URL(`http://${authority}`);
  } catch {
    return undefined;
  }
}

/** Whether a normalized URL hostname names the local loopback authority. */
export function isLoopbackHostname(hostname) {
  if (hostname === "localhost" || hostname === "[::1]") return true;
  const parts = hostname.split(".");
  return (
    parts.length === 4 &&
    parts[0] === "127" &&
    parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255)
  );
}

/** Canonical authority form: hostname, or hostname:port when a port was written. */
function canonicalAuthority(entry, entryUrl) {
  const port =
    entryUrl.port !== ""
      ? entryUrl.port
      : new URL(`https://${entry}`).port;
  return port === "" ? entryUrl.hostname : `${entryUrl.hostname}:${port}`;
}

/** Whether the request authority matches a trustedHosts entry (exact or port-less). */
function isTrustedAuthority(hostUrl, trustedHosts) {
  return trustedHosts.some((entry) => {
    const entryUrl = parseAuthority(entry);
    if (entryUrl === undefined) return false;
    return canonicalAuthority(entry, entryUrl) === entryUrl.hostname
      ? entryUrl.hostname === hostUrl.hostname
      : entryUrl.host === hostUrl.host;
  });
}

/**
 * Decide whether one request may reach the plugin's routes.
 * @param request - node HTTP request facts (headers).
 * @param trustedHosts - non-loopback authorities this deployment serves.
 * @returns true when the Host is ours (loopback or trusted) and browser
 *  markers are same-origin.
 */
export function isTrustedApiRequest(request, trustedHosts) {
  const host = header(request.headers, "host");
  if (host === undefined) return false;
  const hostUrl = parseAuthority(host);
  if (hostUrl === undefined) return false;
  if (!isLoopbackHostname(hostUrl.hostname) && !isTrustedAuthority(hostUrl, trustedHosts)) {
    return false;
  }
  if (header(request.headers, "sec-fetch-site") === "cross-site") return false;
  const origin = header(request.headers, "origin");
  if (origin === undefined) return true;
  try {
    return new URL(origin).host === hostUrl.host;
  } catch {
    return false;
  }
}
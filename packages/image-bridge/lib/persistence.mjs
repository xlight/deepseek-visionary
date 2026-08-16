// ImagePersistence for the visionary image bridge.
//
// Persists image bytes (read back from the attachment service) into a bridge
// directory so the agent can pass the path to `deepseek_vision`. Security and
// lifecycle contract (see the change spec):
//   - directory 0700, files 0600 — created with those modes and re-tightened
//     on every access (the directory holds image copies; privacy wins over a
//     pre-existing looser mode on a user-configured dir);
//   - temp-file + rename atomic write — concurrent first writes of the same
//     content-addressed filename never interleave into a truncated file;
//   - filename = sanitized attachmentId + media-type extension — the id is the
//     attachment library's content address, so the same image always maps to
//     the same file (natural dedup);
//   - process-local `Map` cache keyed by attachmentId with an LRU cap (default
//     max 512 entries) — historical images ride every request through
//     llm/stream, so cache hits skip readImage + disk I/O entirely; entries are
//     dropped when the TTL cleanup removes the file they point at, and a dir
//     change (settings hot-reload) self-heals via the dirname check;
//   - lazy TTL cleanup at startup and after each persist: files older than
//     `retainHours` are deleted together with their cache entries; `retainHours
//     <= 0` means keep everything.
//
// Dependency-free (node:fs only) so unit tests run without a node_modules
// install. `attachments` is injected (the real one is `ctx.attachments`).

import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";

/** mediaType -> file extension, per the change spec's mapping. */
export const EXT_BY_MEDIA_TYPE = {
  "image/png": "png",
  "image/jpeg": "jpeg",
  "image/webp": "webp",
  "image/gif": "gif",
};

/** LRU cap on the attachmentId -> path cache (change task 3.1). */
export const DEFAULT_MAX_CACHE_ENTRIES = 512;

/** Expand a leading `~` in a configured directory to the home directory. */
export function expandHome(dir) {
  if (dir === "~") return os.homedir();
  if (typeof dir === "string" && dir.startsWith("~/")) {
    return path.join(os.homedir(), dir.slice(2));
  }
  return dir;
}

/**
 * One stable, filesystem-safe filename for an image attachment reference.
 * The attachmentId is the content address (e.g. `sha256:<hex>`); sanitizing it
 * keeps the filename stable per image (dedup) while staying cross-platform.
 */
export function attachmentFilename(ref) {
  const safeId = String(ref.attachmentId).replace(/[^A-Za-z0-9_.-]/g, "_");
  const ext = EXT_BY_MEDIA_TYPE[ref.mediaType] ?? "img";
  return `${safeId}.${ext}`;
}

export class ImagePersistence {
  /**
   * @param attachments - image attachment store exposing readImage(ref, signal)
   *   -> { ref, data: Uint8Array } (the real one is ctx.attachments).
   * @param getDir - () => configured pastedDir (may change via settings).
   * @param getRetainHours - () => configured retention in hours.
   * @param logger - optional logger for cleanup warnings.
   * @param maxCacheEntries - LRU cap (default 512).
   */
  constructor({ attachments, getDir, getRetainHours, logger, maxCacheEntries = DEFAULT_MAX_CACHE_ENTRIES }) {
    this.attachments = attachments;
    this.getDir = getDir;
    this.getRetainHours = getRetainHours;
    this.logger = logger;
    this.maxCacheEntries = maxCacheEntries;
    /** @type {Map<string, string>} attachmentId -> absolute path (LRU). */
    this.cache = new Map();
    this.dirReady = null;
  }

  /** Resolve the current absolute bridge directory (expanding `~`). */
  get dir() {
    return expandHome(this.getDir());
  }

  /**
   * Persist one image block's bytes and return the absolute path for the
   * guide. Cache hit skips readImage + disk write. Storage errors surface to
   * the caller (the stream listener degrades them to a text placeholder).
   */
  async persist(ref, signal) {
    const id = String(ref.attachmentId);
    const cached = this.resolveCached(id);
    if (cached !== null) return cached;
    const stored = await this.attachments.readImage(ref, signal);
    const canonical = stored?.ref ?? ref;
    const target = path.join(this.dir, attachmentFilename(canonical));
    await this.ensureDir();
    await this.writeAtomic(target, stored.data);
    this.setCached(id, target);
    await this.lazyCleanup();
    return target;
  }

  /** Create the bridge directory (0700) and re-tighten its mode on access. */
  async ensureDir() {
    const dir = this.dir;
    if (this.dirReady === dir) return;
    await fs.mkdir(dir, { recursive: true, mode: 0o700 });
    // Privacy contract: image copies live here. Best-effort re-tighten even
    // when the dir pre-existed (a user-configured dir gets the same 0700).
    await fs.chmod(dir, 0o700).catch(() => {});
    this.dirReady = dir;
  }

  /** temp-file + rename atomic write with 0600 file mode. */
  async writeAtomic(target, data) {
    // A random suffix keeps concurrent first-writes of the same content
    // addressed file from colliding on one tmp name (same-pid same-ms races).
    const tmp = path.join(
      this.dir,
      `.${path.basename(target)}.${process.pid}.${Date.now()}.${Math.random().toString(36).slice(2)}.tmp`,
    );
    try {
      await fs.writeFile(tmp, data, { mode: 0o600 });
      await fs.rename(tmp, target);
      await fs.chmod(target, 0o600).catch(() => {});
    } finally {
      await fs.rm(tmp, { force: true }).catch(() => {});
    }
  }

  /** LRU read: refresh recency on hit; self-heal on a changed directory. */
  resolveCached(id) {
    const hit = this.cache.get(id);
    if (hit === undefined) return null;
    if (path.dirname(hit) !== this.dir) {
      this.cache.delete(id);
      return null;
    }
    this.cache.delete(id);
    this.cache.set(id, hit);
    return hit;
  }

  /** LRU write: evict the oldest entry beyond the cap. */
  setCached(id, target) {
    if (this.cache.has(id)) this.cache.delete(id);
    this.cache.set(id, target);
    while (this.cache.size > this.maxCacheEntries) {
      const oldest = this.cache.keys().next().value;
      this.cache.delete(oldest);
    }
  }

  /**
   * Lazy TTL cleanup: run at plugin startup and after each persist. Deletes
   * files older than `retainHours` and drops the matching cache entries so no
   * guide ever references a cleaned file. `retainHours <= 0` keeps everything.
   */
  async lazyCleanup() {
    const retainHours = Number(this.getRetainHours());
    if (!Number.isFinite(retainHours) || retainHours <= 0) return;
    const cutoff = Date.now() - retainHours * 3_600_000;
    const dir = this.dir;
    try {
      await fs.access(dir);
    } catch {
      return; // nothing persisted yet
    }
    try {
      const entries = await fs.readdir(dir, { withFileTypes: true });
      for (const entry of entries) {
        if (!entry.isFile()) continue;
        const full = path.join(dir, entry.name);
        try {
          const stat = await fs.stat(full);
          if (stat.mtimeMs < cutoff) await fs.rm(full, { force: true });
        } catch {
          // raced or unreadable; leave it for a later pass
        }
      }
    } catch (err) {
      this.logger?.warn?.(`[visionary-image-bridge] cleanup scan failed: ${err?.message ?? err}`);
      return;
    }
    for (const [id, target] of [...this.cache]) {
      if (path.dirname(target) !== dir) {
        this.cache.delete(id);
        continue;
      }
      try {
        const stat = await fs.stat(target);
        if (stat.mtimeMs < cutoff) this.cache.delete(id);
      } catch {
        this.cache.delete(id); // file already gone
      }
    }
  }
}

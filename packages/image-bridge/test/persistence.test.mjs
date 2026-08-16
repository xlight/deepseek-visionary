// Unit tests for lib/persistence.mjs (change tasks 3.1, 3.6 storage half, 5.1).
// Uses real fs against a mkdtemp directory. Dependency-free.

import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  DEFAULT_MAX_CACHE_ENTRIES,
  ImagePersistence,
  attachmentFilename,
  expandHome,
} from "../lib/persistence.mjs";

const tmpRoots = [];
async function tmpDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "vib-test-"));
  tmpRoots.push(dir);
  return dir;
}

test.after(async () => {
  await Promise.all(tmpRoots.map((d) => fs.rm(d, { recursive: true, force: true })));
});

const ref = (id = "sha256:abc", mediaType = "image/png") => ({
  attachmentId: id,
  mediaType,
  bytes: 3,
  width: 1,
  height: 1,
});

function makePersistence({ pastedDir, retainHours = 168, maxCacheEntries, attachments } = {}) {
  const state = { pastedDir, retainHours };
  return {
    p: new ImagePersistence({
      attachments: attachments ?? {
        readImage: async (r) => ({ ref: r, data: new Uint8Array([1, 2, 3]) }),
      },
      getDir: () => state.pastedDir,
      getRetainHours: () => state.retainHours,
      logger: { warn: () => {} },
      maxCacheEntries,
    }),
    state,
  };
}

test("attachmentFilename: sanitized id + media-type extension", () => {
  assert.equal(attachmentFilename(ref("sha256:abc", "image/png")), "sha256_abc.png");
  assert.equal(attachmentFilename(ref("sha256:abc", "image/jpeg")), "sha256_abc.jpeg");
  assert.equal(attachmentFilename(ref("sha256:abc", "image/webp")), "sha256_abc.webp");
  assert.equal(attachmentFilename(ref("sha256:abc", "image/gif")), "sha256_abc.gif");
  assert.equal(attachmentFilename(ref("weird/id?", "image/png")), "weird_id_.png");
  assert.equal(attachmentFilename(ref("sha256:abc", "image/avif")), "sha256_abc.img");
});

test("expandHome", () => {
  assert.equal(expandHome("~"), os.homedir());
  assert.equal(expandHome("~/x/y"), path.join(os.homedir(), "x", "y"));
  assert.equal(expandHome("/abs/path"), "/abs/path");
});

test("persist: writes bytes with 0600 file in 0700 dir, returns absolute path", async () => {
  const pastedDir = await tmpDir();
  const { p } = makePersistence({ pastedDir });
  const target = await p.persist(ref());
  assert.ok(target.startsWith(pastedDir));
  assert.equal(await fs.readFile(target, "utf8"), "\u0001\u0002\u0003");
  assert.equal((await fs.stat(target)).mode & 0o777, 0o600);
  assert.equal((await fs.stat(pastedDir)).mode & 0o777, 0o700);
  assert.ok((await fs.readdir(pastedDir)).includes("sha256_abc.png"));
});

test("persist: cache hit skips readImage and disk rewrite (idempotent)", async () => {
  const pastedDir = await tmpDir();
  let reads = 0;
  const { p } = makePersistence({
    pastedDir,
    attachments: {
      readImage: async (r) => {
        reads += 1;
        return { ref: r, data: new Uint8Array([1, 2, 3]) };
      },
    },
  });
  const a = await p.persist(ref("sha256:same"));
  const b = await p.persist(ref("sha256:same"));
  assert.equal(a, b);
  assert.equal(reads, 1);
  assert.equal((await fs.readdir(pastedDir)).length, 1); // dedup: one file
});

test("persist: atomic write under concurrent first-writes of the same id", async () => {
  const pastedDir = await tmpDir();
  let reads = 0;
  const { p } = makePersistence({
    pastedDir,
    attachments: {
      readImage: async (r) => {
        reads += 1;
        // simulate slow read so both requests race through persist
        await new Promise((res) => setTimeout(res, 10));
        return { ref: r, data: new Uint8Array([9, 9, 9]) };
      },
    },
  });
  const [a, b] = await Promise.all([p.persist(ref("sha256:race")), p.persist(ref("sha256:race"))]);
  assert.equal(a, b);
  assert.deepEqual(new Uint8Array(await fs.readFile(a)), new Uint8Array([9, 9, 9])); // not truncated
});

test("persist: LRU eviction beyond cap", async () => {
  const pastedDir = await tmpDir();
  const { p } = makePersistence({ pastedDir, maxCacheEntries: 2 });
  const p1 = await p.persist(ref("sha256:one"));
  const p2 = await p.persist(ref("sha256:two"));
  await p.persist(ref("sha256:three")); // evicts sha256:one
  assert.equal(p.resolveCached("sha256:one"), null);
  assert.equal(p.resolveCached("sha256:two"), p2);
  assert.ok((await fs.readdir(pastedDir)).length === 3); // files stay; only cache evicted
  assert.ok(p1);
});

test("cache: dir change self-heals (settings hot-reload of pastedDir)", async () => {
  const dirA = await tmpDir();
  const dirB = await tmpDir();
  const { p, state } = makePersistence({ pastedDir: dirA });
  const a = await p.persist(ref("sha256:move"));
  state.pastedDir = dirB;
  assert.equal(p.resolveCached("sha256:move"), null); // stale entry dropped
  const b = await p.persist(ref("sha256:move"));
  assert.notEqual(b, a);
  assert.ok(b.startsWith(dirB));
});

test("lazyCleanup: deletes expired files and matching cache entries", async () => {
  const pastedDir = await tmpDir();
  const { p, state } = makePersistence({ pastedDir, retainHours: 1 }); // 1h retention
  await p.persist(ref("sha256:old"));
  await p.persist(ref("sha256:new"));
  assert.equal(p.resolveCached("sha256:old") !== null, true);

  const old = path.join(pastedDir, "sha256_old.png");
  const past = new Date(Date.now() - 10 * 3_600_000); // 10h ago > 1h retention
  await fs.utimes(old, past, past);

  await p.lazyCleanup();
  assert.equal((await fs.readdir(pastedDir)).length, 1); // only sha256_new.png
  assert.equal(p.resolveCached("sha256:old"), null); // cache entry synced
  assert.equal(p.resolveCached("sha256:new") !== null, true);
});

test("lazyCleanup: retainHours <= 0 keeps everything", async () => {
  const pastedDir = await tmpDir();
  const { p, state } = makePersistence({ pastedDir, retainHours: 0 });
  await p.persist(ref("sha256:keep"));
  const past = new Date(Date.now() - 30 * 3_600_000);
  await fs.utimes(path.join(pastedDir, "sha256_keep.png"), past, past);
  await p.lazyCleanup();
  assert.equal((await fs.readdir(pastedDir)).length, 1);
  assert.ok(p.resolveCached("sha256:keep"));
  state.retainHours = -1;
  await p.lazyCleanup();
  assert.equal((await fs.readdir(pastedDir)).length, 1);
});

test("DEFAULT_MAX_CACHE_ENTRIES is 512", () => {
  assert.equal(DEFAULT_MAX_CACHE_ENTRIES, 512);
});

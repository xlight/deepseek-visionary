// Unit tests for lib/rewrite.mjs (change task 3.6: rewrite correctness and
// message fidelity). Dependency-free — runs without node_modules.

import test from "node:test";
import assert from "node:assert/strict";
import {
  contentHasImage,
  messagesHaveImage,
  renderGuide,
  rewriteMessages,
} from "../lib/rewrite.mjs";

const img = (id = "sha256:abc") => ({
  type: "image",
  attachment: { attachmentId: id, mediaType: "image/png", bytes: 3, width: 1, height: 1 },
});

const text = (t) => ({ type: "text", text: t });

const userMsg = (content, extra = {}) => ({
  id: "m1",
  role: "user",
  content,
  source: { kind: "user" },
  ...extra,
});

const guide = (block) => `GUIDE:${block.attachment.attachmentId}`;

test("contentHasImage: flat and nested (tool-result)", () => {
  assert.equal(contentHasImage([text("hi")]), false);
  assert.equal(contentHasImage([img()]), true);
  assert.equal(
    contentHasImage([{ type: "tool-result", toolCallId: "c1", content: [text("x"), img()] }]),
    true,
  );
  assert.equal(
    contentHasImage([{ type: "tool-result", toolCallId: "c1", content: [text("x")] }]),
    false,
  );
});

test("messagesHaveImage", () => {
  assert.equal(messagesHaveImage([userMsg([text("hi")])]), false);
  assert.equal(messagesHaveImage([userMsg([img()])]), true);
});

test("renderGuide replaces every {path} literally", () => {
  assert.equal(renderGuide("saved at {path}, {path} again", "/a/b.png"), "saved at /a/b.png, /a/b.png again");
  assert.equal(renderGuide("no placeholder", "/a/b.png"), "no placeholder");
});

test("rewriteMessages: single image -> guide; message cloned, fidelity preserved", async () => {
  const original = userMsg([text("问题?"), img("sha256:abc")], {
    source: { kind: "model", provider: "pi-ai", model: "flash", replayState: { k: 1 } },
  });
  const other = userMsg([text("保持原样")]);
  const out = await rewriteMessages([original, other], guide);

  assert.notEqual(out, [original, other]); // changed array
  assert.notEqual(out[0], original); // image message cloned
  assert.equal(out[0].id, "m1"); // id preserved
  assert.deepEqual(out[0].source, original.source); // source incl. replayState preserved
  assert.deepEqual(out[0].content, [text("问题?"), text("GUIDE:sha256:abc")]); // text kept, image -> guide
  assert.equal(out[1], other); // non-image message keeps original reference
});

test("rewriteMessages: multi-image ordered guides, non-image blocks preserved", async () => {
  const original = userMsg([img("sha256:a"), text("中间文本"), img("sha256:b")]);
  const out = await rewriteMessages([original], guide);
  assert.deepEqual(out[0].content, [
    text("GUIDE:sha256:a"),
    text("中间文本"),
    text("GUIDE:sha256:b"),
  ]);
});

test("rewriteMessages: tool-result nesting rewritten, call identity preserved", async () => {
  const original = userMsg([
    {
      type: "tool-result",
      toolCallId: "call-1",
      isError: false,
      content: [text("输出:"), img("sha256:tool")],
    },
  ]);
  const out = await rewriteMessages([original], guide);
  const block = out[0].content[0];
  assert.equal(block.type, "tool-result");
  assert.equal(block.toolCallId, "call-1");
  assert.equal(block.isError, false);
  assert.deepEqual(block.content, [text("输出:"), text("GUIDE:sha256:tool")]);
});

test("rewriteMessages: no images -> returns the original array identity", async () => {
  const messages = [userMsg([text("plain")])];
  const out = await rewriteMessages(messages, guide);
  assert.equal(out, messages);
});

test("rewriteMessages: resolver failure propagates (listener degrades separately)", async () => {
  const messages = [userMsg([img()])];
  await assert.rejects(
    rewriteMessages(messages, async () => {
      throw new Error("boom");
    }),
    /boom/,
  );
});

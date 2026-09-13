// Browser-half tests for lib/client.js (the Settings → Plugins cards).
//
// The shipped artifact is a hand-written lazy-CJS bundle, so the test evaluates
// it exactly as the DSH module loader would (a `window.__ModuleLoader__.load`
// call), then drives the real components through a tiny React hook harness:
// state, effects, and element trees are enough to exercise the card surface
// without a DOM.
//
// Covered: the bundle contract (id = package name, `react`-only module edge),
// the two keyed `settings.plugin.item` registrations, staged edits written
// through `settingsScope.mutate` with the read revision as the fence, the
// shared `binaryPath` writing both namespaces, the collapse of the advanced
// region, the one-shot `cleanPasted` trigger, reset via `unset`, stale-write
// conflict reporting, and the loading/unavailable/read-only degradation paths.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BUNDLE = fileURLToPath(new URL("../lib/client.js", import.meta.url));
// The registered id must be the package name: the runner activates the row by
// requiring it (see the discovery test in integration-smoke.test.mjs).
const BUNDLE_ID = JSON.parse(readFileSync(fileURLToPath(new URL("../package.json", import.meta.url)), "utf8")).name;
const NS_VISION = "visionary-vision";
const NS_BRIDGE = "visionary-image-bridge";
/** Platform seed module the host exposes to every card bundle. */
const PRIMITIVES_ID = "@deepseek-ai/dsh-client-ui-primitives";
/** Stand-in for the host Switch: the tiny renderer does not expand components,
 *  so the element type stays this function and assertions match on it. */
const SwitchStub = (props) => ({ type: "switch", props });

// ── minimal React ───────────────────────────────────────────────────────────

function makeReact() {
  const hooks = { values: [], effects: [] };
  let cursor = 0;
  let pending = [];
  let rerender = () => {};
  const React = {
    createElement(type, props, ...children) {
      const merged = Object.assign({}, props || {});
      if (children.length === 1) merged.children = children[0];
      else if (children.length > 1) merged.children = children;
      return { type, props: merged };
    },
    useState(init) {
      const i = cursor++;
      if (!(i in hooks.values)) hooks.values[i] = typeof init === "function" ? init() : init;
      const set = (next) => {
        hooks.values[i] = typeof next === "function" ? next(hooks.values[i]) : next;
        rerender();
      };
      return [hooks.values[i], set];
    },
    useEffect(fn, deps) {
      const i = cursor++;
      const prev = hooks.effects[i];
      const same = prev !== undefined
        && Array.isArray(deps) && Array.isArray(prev.deps)
        && deps.length === prev.deps.length
        && deps.every((d, k) => d === prev.deps[k]);
      if (!same) pending.push({ i, fn, deps });
    },
    useRef(init) {
      const i = cursor++;
      if (!(i in hooks.values)) hooks.values[i] = { current: init };
      return hooks.values[i];
    },
  };
  const render = (Component) => {
    cursor = 0;
    pending = [];
    const tree = Component({});
    pending.forEach(({ i, fn, deps }) => { hooks.effects[i] = { fn, deps }; fn(); });
    return tree;
  };
  return { React, render, setRerender: (fn) => { rerender = fn; } };
}

function walk(node, visit) {
  if (node === null || node === undefined || typeof node === "boolean") return;
  if (Array.isArray(node)) { node.forEach((child) => walk(child, visit)); return; }
  if (typeof node !== "object") return;
  visit(node);
  walk(node.props ? node.props.children : undefined, visit);
}

function collect(tree, predicate) {
  const out = [];
  walk(tree, (node) => { if (predicate(node)) out.push(node); });
  return out;
}

function textOf(node) {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (node && node.props) return textOf(node.props.children);
  return "";
}

function buttons(tree, label) {
  return collect(tree, (n) => n.type === "button" && textOf(n).includes(label) === true);
}

/** Cards carry the host's own class names, so assertions can target them. */
function byClass(tree, className) {
  return collect(tree, (n) => n.type === "button" && n.props.className === className);
}

// ── scope + ctx fakes ───────────────────────────────────────────────────────

function makeScope(namespace, { value, user = undefined, revision = 7, status = "ready", writable = true } = {}) {
  const state = {
    status,
    value: Object.assign({}, value),
    base: Object.assign({}, value),
    user,
    revision,
    writable,
    mode: "host",
  };
  const listeners = new Set();
  const scope = {
    namespace,
    state,
    calls: { mutate: [], set: [], unset: [] },
    rejectNext: false,
    getSnapshot: () => state,
    subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    emit: () => { listeners.forEach((l) => l()); },
    mutate(ops, expectedRevision) {
      scope.calls.mutate.push({ ops, expectedRevision });
      if (scope.rejectNext) { scope.rejectNext = false; return Promise.resolve(); }
      const next = Object.assign({}, state.value);
      ops.forEach((op) => {
        if (op.op === "unset") delete next[op.path[0]];
        else next[op.path[0]] = op.value;
      });
      state.value = next;
      state.user = Object.assign({}, state.user, ...ops.map((op) => (op.op === "set" ? { [op.path[0]]: op.value } : {})));
      state.revision = expectedRevision === undefined ? state.revision : expectedRevision + 1;
      scope.emit();
      return Promise.resolve();
    },
    set(field, value) {
      scope.calls.set.push({ field, value });
      return scope.mutate([{ op: "set", path: [field], value }], state.revision);
    },
    unset(field) {
      scope.calls.unset.push(field);
      const next = Object.assign({}, state.value);
      delete next[field];
      state.value = next;
      if (state.user) { const u = Object.assign({}, state.user); delete u[field]; state.user = u; }
      scope.emit();
      return Promise.resolve();
    },
  };
  return scope;
}

/** Evaluate the shipped bundle the way the loader does. */
function loadBundle() {
  let captured = null;
  const fakeWindow = { __ModuleLoader__: { load: (options) => { captured = options; } } };
  // eslint-disable-next-line no-new-func
  new Function("window", readFileSync(BUNDLE, "utf8"))(fakeWindow);
  assert.ok(captured, "the bundle must call window.__ModuleLoader__.load");
  const required = [];
  const react = makeReact();
  // The host Switch primitive: identity only, so the card's use of it is visible.
  const primitives = { Switch: SwitchStub };
  const exportsObj = captured.factory((id) => {
    required.push(id);
    if (id === "react") return react.React;
    if (id === PRIMITIVES_ID) return primitives;
    throw new Error(`unexpected module request: ${id}`);
  });
  return { id: captured.id, exports: exportsObj, required, react, primitives };
}

/** Boot the browser plugin against fake scopes; returns the two card handles. */
function bootCard({ vision, bridge } = {}) {
  const bundle = loadBundle();
  const scopes = {
    [NS_VISION]: makeScope(NS_VISION, { value: Object.assign({
      binaryPath: "",
      modelType: "vision",
      visionTimeoutMs: 1111,
      statusTimeoutMs: 2222,
      loginTimeoutSeconds: 3333,
    }, vision && vision.value), user: vision && vision.user, status: vision && vision.status, writable: vision && vision.writable, revision: 7 }),
    [NS_BRIDGE]: makeScope(NS_BRIDGE, { value: Object.assign({
      binaryPath: "",
      enabled: true,
      scope: "text-only",
      mode: "agentic",
      promptTemplate: "path {path}",
      pastedDir: "~/.deepseek-visionary/pasted",
      retainHours: 168,
      cleanPasted: false,
    }, bridge && bridge.value), user: bridge && bridge.user, revision: 11 }),
  };
  const registrations = [];
  const injections = [];
  const effects = [];
  const ctx = {
    locale: { register: () => () => {}, bind: () => (key) => key },
    settingsScope: { bind: (spec) => scopes[spec.namespace] },
    slots: {
      inject: (name, callback) => { injections.push({ name, callback }); },
      register: (options, component) => { registrations.push({ options, component }); return () => {}; },
    },
    effect: (fn) => { effects.push(fn()); return () => {}; },
  };
  bundle.exports.apply(ctx);
  injections.forEach(({ callback }) => callback());

  const cards = {};
  registrations.forEach(({ options, component }) => { cards[options.key] = { options, component }; });

  const render = (namespace, { open = true } = {}) => {
    let tree = null;
    const draw = () => { tree = bundle.react.render(cards[namespace].component); };
    bundle.react.setRerender(draw);
    draw();
    // Native cards start collapsed; most assertions want the body. Expansion is
    // idempotent (hook state survives a re-render), and `open: false` observes the
    // card exactly as it is, so collapse behaviour stays testable.
    const body = collect(tree, (n) => n.props && n.props.className === "vlb-body")[0];
    if (open && !body) {
      const header = collect(tree, (n) => n.type === "button" && n.props.className === "vlb-header")[0];
      if (header) header.props.onClick();
    }
    return () => tree;
  };

  return { bundle, scopes, registrations, cards, render, effects };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

// ── bundle contract ─────────────────────────────────────────────────────────

test("bundle: loader id and the module edges it needs", () => {
  const { id, required } = loadBundle();
  assert.equal(id, BUNDLE_ID);
  assert.deepEqual(required, ["react", PRIMITIVES_ID],
    "the card needs the react seed word and the host Switch primitive");
});

test("bundle: no reference to the removed client-runtime module", () => {
  const source = readFileSync(BUNDLE, "utf8");
  assert.ok(!source.includes("@deepseek-ai/dsh-client-runtime"), "the runtime edge must be gone");
  assert.ok(!source.includes("createSnapshotStore"), "the hand-written snapshot generation layer must be gone");
  assert.ok(!source.includes("/visionary/api"), "the private settings route must be gone");
});

test("bundle: injects the slots, locale and settingsScope services", () => {
  const { exports } = loadBundle();
  assert.deepEqual(exports.inject, ["slots", "locale", "settingsScope"]);
});

// ── registration ────────────────────────────────────────────────────────────

test("registration: one settings.plugin.item card per namespace", () => {
  const { registrations, cards } = bootCard();
  assert.equal(registrations.length, 2);
  for (const options of registrations.map((r) => r.options)) {
    assert.equal(options.name, "settings.plugin.item");
    assert.equal(typeof options.order, "number");
  }
  assert.deepEqual(Object.keys(cards).sort(), [NS_BRIDGE, NS_VISION].sort());
  assert.equal(typeof cards[NS_VISION].component, "function");
  assert.equal(typeof cards[NS_BRIDGE].component, "function");
});

// ── writes ──────────────────────────────────────────────────────────────────

test("write: a staged number is written through mutate with the read revision", async () => {
  const { scopes, render } = bootCard();
  let tree = render(NS_VISION)();
  const input = collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0];
  assert.ok(input, "the vision timeout field renders its current value");
  input.props.onChange({ target: { value: "9999" } });

  tree = render(NS_VISION)();
  const save = byClass(tree, "vlb-save")[0];
  assert.ok(save, "the save button renders");
  assert.equal(save.props.disabled, false, "a dirty, valid card may save");
  save.props.onClick();
  await tick();

  assert.equal(scopes[NS_VISION].calls.mutate.length, 1);
  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [
    { op: "set", path: ["visionTimeoutMs"], value: 9999 },
  ]);
  assert.equal(scopes[NS_VISION].calls.mutate[0].expectedRevision, 7, "the read revision fences the write");
  assert.equal(scopes[NS_BRIDGE].calls.mutate.length, 0, "an unshared field touches one namespace only");
});

test("write: the shared binaryPath writes both namespaces with their own revisions", async () => {
  const { scopes, render } = bootCard();
  let tree = render(NS_VISION)();
  const input = collect(tree, (n) => n.type === "input" && n.props.value === "")[0];
  assert.ok(input, "the empty binary path field renders");
  input.props.onChange({ target: { value: "/opt/visionary-server" } });

  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();

  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [
    { op: "set", path: ["binaryPath"], value: "/opt/visionary-server" },
  ]);
  assert.equal(scopes[NS_VISION].calls.mutate[0].expectedRevision, 7);
  assert.deepEqual(scopes[NS_BRIDGE].calls.mutate[0].ops, [
    { op: "set", path: ["binaryPath"], value: "/opt/visionary-server" },
  ]);
  assert.equal(scopes[NS_BRIDGE].calls.mutate[0].expectedRevision, 11);
});

test("write: clearing a field sends unset", async () => {
  const { scopes, render } = bootCard();
  let tree = render(NS_VISION)();
  collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0]
    .props.onChange({ target: { value: "" } });
  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [
    { op: "unset", path: ["visionTimeoutMs"] },
  ]);
});

test("write: a rejected write surfaces as the conflict notice and re-reads", async () => {
  const { scopes, render } = bootCard();
  scopes[NS_VISION].rejectNext = true;
  let tree = render(NS_VISION)();
  collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0]
    .props.onChange({ target: { value: "9999" } });
  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  await tick();
  tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("saveConflict"), "the card reports a refused write");
});

test("write: the one-shot cleanPasted trigger writes true and reports it", async () => {
  const { scopes, render } = bootCard();
  let tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-disclosure")[0].props.onClick();
  tree = render(NS_BRIDGE)();
  const trigger = byClass(tree, "vlb-trigger")[0];
  assert.ok(trigger, "the trigger button lives in the advanced region");
  trigger.props.onClick();
  await tick();
  assert.deepEqual(scopes[NS_BRIDGE].calls.mutate[0].ops, [
    { op: "set", path: ["cleanPasted"], value: true },
  ]);
  assert.equal(scopes[NS_BRIDGE].calls.mutate[0].expectedRevision, 11);
});

test("write: reset clears one overridden field through unset", async () => {
  const { scopes, render } = bootCard({ vision: { value: { modelType: "ocr" }, user: { modelType: "ocr" } } });
  let tree = render(NS_VISION)();
  const reset = byClass(tree, "vlb-reset")[0];
  assert.ok(reset, "an overridden field shows its reset affordance");
  reset.props.onClick();
  await tick();
  assert.deepEqual(scopes[NS_VISION].calls.unset, ["modelType"]);
});

// ── layout and degradation ──────────────────────────────────────────────────

test("layout: advanced fields stay collapsed until the disclosure opens", () => {
  const { render } = bootCard();
  let tree = render(NS_VISION)();
  assert.equal(collect(tree, (n) => n.type === "input" && n.props.value === "2222").length, 0,
    "statusTimeoutMs (advanced) is collapsed");
  byClass(tree, "vlb-disclosure")[0].props.onClick();
  tree = render(NS_VISION)();
  assert.equal(collect(tree, (n) => n.type === "input" && n.props.value === "2222").length, 1,
    "statusTimeoutMs appears once the disclosure opens");
  assert.equal(collect(tree, (n) => n.type === "input" && n.props.value === "1111").length, 1,
    "the main region stays visible");
});

test("layout: the bridge card keeps its own advanced field set", () => {
  const { render } = bootCard();
  let tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-disclosure")[0].props.onClick();
  tree = render(NS_BRIDGE)();
  assert.equal(collect(tree, (n) => n.type === "textarea").length, 1, "promptTemplate renders in advanced");
  assert.equal(collect(tree, (n) => n.type === "select").length, 2, "scope and mode stay in the main region");
});

test("degradation: an unavailable namespace renders a notice instead of throwing", () => {
  const { render } = bootCard({ vision: { status: "unavailable" } });
  const tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("unavailable"));
  assert.equal(collect(tree, (n) => n.type === "input").length, 0);
});

test("degradation: a read-only deployment disables editing", () => {
  const { render } = bootCard({ vision: { writable: false } });
  const tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("readOnly"));
  const inputs = collect(tree, (n) => n.type === "input" || n.type === "select");
  assert.ok(inputs.length > 0);
  assert.ok(inputs.every((n) => n.props.disabled === true), "every control is disabled");
});

// ── host-native chrome ──────────────────────────────────────────────────────

test("chrome: apply() injects one tagged stylesheet and removes it on dispose", () => {
  const appended = [];
  const removed = [];
  const style = {
    textContent: "",
    attributes: {},
    setAttribute(name, value) { this.attributes[name] = value; },
    remove() { removed.push(true); },
  };
  globalThis.document = {
    head: { appendChild: (el) => appended.push(el) },
    createElement: () => style,
  };
  try {
    const { effects } = bootCard();
    assert.equal(appended.length, 1, "exactly one stylesheet for both cards");
    assert.ok(style.textContent.includes(".vlb-card"), "the card chrome ships in the bundle");
    assert.equal(style.attributes["data-plugin"], BUNDLE_ID);
    const dispose = effects[1];
    assert.equal(typeof dispose, "function", "the stylesheet effect returns a disposer");
    dispose();
    assert.equal(removed.length, 1, "dispose removes the stylesheet");
  } finally {
    delete globalThis.document;
  }
});

test("chrome: the card is a native collapsible row, closed until its header is clicked", () => {
  const { render } = bootCard();
  const collapsed = render(NS_VISION, { open: false })();
  const root = collect(collapsed, (n) => n.type === "li")[0];
  assert.ok(root, "the card root is a list item, like the host's own cards");
  assert.equal(root.props.className, "vlb-card");
  assert.equal(textOf(collapsed).includes("visionTitle"), true, "the header names the card while closed");
  assert.equal(collect(collapsed, (n) => n.type === "input").length, 0, "no fields while collapsed");
  const opened = render(NS_VISION)();
  assert.ok(collect(opened, (n) => n.type === "input").length > 0, "fields appear while open");
  const openRoot = collect(opened, (n) => n.type === "li")[0];
  assert.equal(openRoot.props.className, "vlb-card vlb-cardOpen");
});

test("chrome: the header carries the host's accessible expand/collapse contract", () => {
  const { render } = bootCard();
  const closed = render(NS_VISION, { open: false })();
  const closedHeader = collect(closed, (n) => n.type === "button" && n.props.className === "vlb-header")[0];
  assert.equal(closedHeader.props["aria-expanded"], "false");
  assert.equal(closedHeader.props["aria-label"], "expand: visionTitle");
  const open = render(NS_VISION)();
  const openHeader = collect(open, (n) => n.type === "button" && n.props.className === "vlb-header")[0];
  assert.equal(openHeader.props["aria-expanded"], "true");
  assert.equal(openHeader.props["aria-label"], "collapse: visionTitle");
});

test("chrome: a dirty card flags itself as unsaved, and a completed save collapses it", async () => {
  const { render } = bootCard();
  let tree = render(NS_VISION)();
  assert.equal(textOf(tree).includes("unsaved"), false, "a clean card shows no unsaved flag");
  collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0]
    .props.onChange({ target: { value: "9999" } });
  tree = render(NS_VISION)();
  assert.equal(textOf(tree).includes("unsaved"), true, "a staged edit flags the card");
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  await tick();
  tree = render(NS_VISION, { open: false })();
  assert.equal(collect(tree, (n) => n.type === "input").length, 0);
  assert.equal(textOf(tree).includes("unsaved"), false, "the flag clears with the write");
});

test("chrome: boolean fields render the host Switch, not a raw checkbox", async () => {
  const { scopes, render } = bootCard();
  let tree = render(NS_BRIDGE)();
  const toggle = collect(tree, (n) => n.type === SwitchStub)[0];
  assert.ok(toggle, "the bridge enabled field renders the host Switch primitive");
  assert.equal(toggle.props.checked, true);
  assert.equal(toggle.props.label, "enabledLabel");
  assert.equal(collect(tree, (n) => n.type === "input" && n.props.type === "checkbox").length, 0,
    "no browser-default checkbox is left in the card");
  toggle.props.onChange(false);
  await tick();
  tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  assert.deepEqual(scopes[NS_BRIDGE].calls.mutate[0].ops, [
    { op: "set", path: ["enabled"], value: false },
  ]);
});

test("chrome: field controls are labelled and marked invalid for assistive tech", () => {
  const { render } = bootCard();
  let tree = render(NS_VISION)();
  const labels = collect(tree, (n) => n.type === "label" && n.props.htmlFor);
  assert.ok(labels.length >= 3, "every field carries a label bound to its control id");
  for (const label of labels) {
    const control = collect(tree, (n) => ["input", "select", "textarea"].includes(n.type)
      && n.props.id === label.props.htmlFor)[0];
    assert.ok(control, `the control for ${label.props.htmlFor} exists`);
  }
  const timed = collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0];
  assert.equal(timed.props["aria-invalid"], "false", "a valid number field is announced as valid");
  timed.props.onChange({ target: { value: "nope" } });
  tree = render(NS_VISION)();
  const invalid = collect(tree, (n) => n.type === "input" && n.props["aria-invalid"] === "true")[0];
  assert.ok(invalid, "an invalid staged value is announced");
  assert.ok(invalid.props.className.includes("vlb-inputInvalid"));
});


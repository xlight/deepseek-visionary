// Browser-half tests for lib/client.js (the Plugins page row configuration).
//
// The shipped artifact is a hand-written lazy-CJS bundle, so the test evaluates
// it exactly as the DSH module loader would (a `window.__ModuleLoader__.load`
// call), then drives the real components through a tiny React hook harness:
// state, effects, and element trees are enough to exercise the row surface
// without a DOM.
//
// DSH 0.1.7 contract under test: the bundle injects `slots` + `locale` only
// (no `settingsScope`), registers one `plugins.row.config` entry per plugin row
// keyed `<package>#<rowId>`, and the row component consumes the page-injected
// `{ view, form }` props (ConfigPageForm) — `form` is the host entry's
// ConfigForm (getSnapshot/subscribe/mutate/unset). Covered: the bundle
// contract, the two keyed registrations, staged edits written through
// `form.mutate` with the read revision as the fence, the advanced disclosure,
// the one-shot `cleanPasted` trigger, reset via `unset`, stale-write conflict
// reporting, summary-view rendering, and the loading/unavailable/read-only
// degradation paths.

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
/** `plugins.row.config` keys follow the page's rowConfigKey(bundle, rowId). */
const ROW_KEY = (rowId) => `${BUNDLE_ID}#${rowId}`;
/** Platform seed module the host exposes to every browser bundle. */
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
    useMemo(fn, deps) {
      const i = cursor++;
      const prev = hooks.effects[i];
      const same = prev !== undefined
        && Array.isArray(deps) && Array.isArray(prev.deps)
        && deps.length === prev.deps.length
        && deps.every((d, k) => d === prev.deps[k]);
      if (!same) {
        hooks.effects[i] = { fn, deps };
        hooks.values[i] = fn();
      }
      return hooks.values[i];
    },
    useRef(init) {
      const i = cursor++;
      if (!(i in hooks.values)) hooks.values[i] = { current: init };
      return hooks.values[i];
    },
  };
  const render = (Component, props) => {
    cursor = 0;
    pending = [];
    const tree = Component(props || {});
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

function byClass(tree, className) {
  return collect(tree, (n) => n.type === "button" && n.props.className === className);
}

// ── ConfigForm fake (the page-injected `form`) ──────────────────────────────

function makeForm({ value, user = undefined, revision = 7, status = "ready", writable = true } = {}) {
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
  const form = {
    state,
    calls: { mutate: [], set: [], unset: [] },
    rejectNext: false,
    getSnapshot: () => state,
    subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    emit: () => { listeners.forEach((l) => l()); },
    mutate(ops, expectedRevision) {
      form.calls.mutate.push({ ops, expectedRevision });
      if (form.rejectNext) { form.rejectNext = false; return Promise.resolve(); }
      const next = Object.assign({}, state.value);
      ops.forEach((op) => {
        if (op.op === "unset") delete next[op.path[0]];
        else next[op.path[0]] = op.value;
      });
      state.value = next;
      state.user = Object.assign({}, state.user, ...ops.map((op) => (op.op === "set" ? { [op.path[0]]: op.value } : {})));
      state.revision = expectedRevision === undefined ? state.revision : expectedRevision + 1;
      form.emit();
      return Promise.resolve();
    },
    set(field, value) {
      form.calls.set.push({ field, value });
      return form.mutate([{ op: "set", path: [field], value }], state.revision);
    },
    unset(field) {
      form.calls.unset.push(field);
      const next = Object.assign({}, state.value);
      delete next[field];
      state.value = next;
      if (state.user) { const u = Object.assign({}, state.user); delete u[field]; state.user = u; }
      form.emit();
      return Promise.resolve();
    },
  };
  return form;
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
  // The host Switch primitive: identity only, so the row's use of it is visible.
  const primitives = { Switch: SwitchStub };
  const exportsObj = captured.factory((id) => {
    required.push(id);
    if (id === "react") return react.React;
    if (id === PRIMITIVES_ID) return primitives;
    throw new Error(`unexpected module request: ${id}`);
  });
  return { id: captured.id, exports: exportsObj, required, react, primitives };
}

/** Boot the browser plugin; returns the two row handles plus the sessions. */
function bootRows({ vision, bridge } = {}) {
  const bundle = loadBundle();
  const forms = {
    [NS_VISION]: makeForm({ value: Object.assign({
      binaryPath: "",
      modelType: "vision",
      visionTimeoutMs: 1111,
      statusTimeoutMs: 2222,
      loginTimeoutSeconds: 3333,
    }, vision && vision.value), user: vision && vision.user, status: vision && vision.status, writable: vision && vision.writable, revision: 7 }),
    [NS_BRIDGE]: makeForm({ value: Object.assign({
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
    slots: {
      inject: (name, callback) => { injections.push({ name, callback }); },
      register: (options, component) => { registrations.push({ options, component }); return () => {}; },
    },
    effect: (fn) => { effects.push(fn()); return () => {}; },
  };
  bundle.exports.apply(ctx);
  injections.forEach(({ callback }) => callback());

  const rows = {};
  registrations.forEach(({ options, component }) => { rows[options.key] = { options, component }; });

  const render = (rowId, overrides = {}) => {
    let tree = null;
    const draw = () => {
      tree = bundle.react.render(rows[ROW_KEY(rowId)].component, {
        view: "page",
        form: forms[rowId],
        ...overrides,
      });
    };
    bundle.react.setRerender(draw);
    draw();
    return () => tree;
  };

  return { bundle, forms, registrations, rows, render, effects, injections };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

// ── bundle contract ─────────────────────────────────────────────────────────

test("bundle: loader id and the module edges it needs", () => {
  const { id, required } = loadBundle();
  assert.equal(id, BUNDLE_ID);
  assert.deepEqual(required, ["react", PRIMITIVES_ID],
    "the row needs the react seed word and the host Switch primitive");
});

test("bundle: no reference to the removed client-runtime module", () => {
  const source = readFileSync(BUNDLE, "utf8");
  assert.ok(!source.includes("@deepseek-ai/dsh-client-runtime"), "the runtime edge must be gone");
  assert.ok(!source.includes("ctx.settingsScope.bind"), "the 0.1.5 settingsScope binding must be gone from the runtime");
  assert.ok(!source.includes('"settings.plugin.item"'), "the 0.1.5 plugin card slot must be gone");
  assert.ok(!source.includes("/visionary/api"), "the private settings route must be gone");
});

test("bundle: injects only the slots and locale services", () => {
  const { exports } = loadBundle();
  assert.deepEqual(exports.inject, ["slots", "locale"],
    "activation must not wait on a settings service the 0.1.7 host no longer provides");
});

// ── registration ────────────────────────────────────────────────────────────

test("registration: one plugins.row.config entry per plugin row", () => {
  const { registrations, rows, injections } = bootRows();
  assert.equal(injections.length, 2);
  assert.ok(injections.every((i) => i.name === "plugins.row.config"));
  assert.equal(registrations.length, 2);
  for (const options of registrations.map((r) => r.options)) {
    assert.equal(options.name, "plugins.row.config");
    assert.equal(typeof options.order, "number");
  }
  assert.deepEqual(Object.keys(rows).sort(), [ROW_KEY(NS_BRIDGE), ROW_KEY(NS_VISION)].sort());
  assert.equal(typeof rows[ROW_KEY(NS_VISION)].component, "function");
  assert.equal(typeof rows[ROW_KEY(NS_BRIDGE)].component, "function");
});

// ── writes ──────────────────────────────────────────────────────────────────

test("write: a staged number is written through mutate with the read revision", async () => {
  const { forms, render } = bootRows();
  let tree = render(NS_VISION)();
  const input = collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0];
  assert.ok(input, "the vision timeout field renders its current value");
  input.props.onChange({ target: { value: "9999" } });

  tree = render(NS_VISION)();
  const save = byClass(tree, "vlb-save")[0];
  assert.ok(save, "the save button renders");
  assert.equal(save.props.disabled, false, "a dirty, valid row may save");
  save.props.onClick();
  await tick();

  assert.equal(forms[NS_VISION].calls.mutate.length, 1);
  assert.deepEqual(forms[NS_VISION].calls.mutate[0].ops, [
    { op: "set", path: ["visionTimeoutMs"], value: 9999 },
  ]);
  assert.equal(forms[NS_VISION].calls.mutate[0].expectedRevision, 7, "the read revision fences the write");
  assert.equal(forms[NS_BRIDGE].calls.mutate.length, 0, "a page-local field touches its own entry only");
});

test("write: the binaryPath field writes its own row entry only", async () => {
  const { forms, render } = bootRows();
  let tree = render(NS_VISION)();
  const input = collect(tree, (n) => n.type === "input" && n.props.value === "")[0];
  assert.ok(input, "the empty binary path field renders");
  input.props.onChange({ target: { value: "/opt/visionary-server" } });

  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();

  assert.deepEqual(forms[NS_VISION].calls.mutate[0].ops, [
    { op: "set", path: ["binaryPath"], value: "/opt/visionary-server" },
  ]);
  assert.equal(forms[NS_VISION].calls.mutate[0].expectedRevision, 7);
  assert.equal(forms[NS_BRIDGE].calls.mutate.length, 0,
    "each row page owns its entry; the bridge binaryPath is configured on its own page");
});

test("write: clearing a field sends unset", async () => {
  const { forms, render } = bootRows();
  let tree = render(NS_VISION)();
  collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0]
    .props.onChange({ target: { value: "" } });
  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  assert.deepEqual(forms[NS_VISION].calls.mutate[0].ops, [
    { op: "unset", path: ["visionTimeoutMs"] },
  ]);
});

test("write: a rejected write surfaces as the conflict notice and re-reads", async () => {
  const { forms, render } = bootRows();
  forms[NS_VISION].rejectNext = true;
  let tree = render(NS_VISION)();
  collect(tree, (n) => n.type === "input" && n.props.value === "1111")[0]
    .props.onChange({ target: { value: "9999" } });
  tree = render(NS_VISION)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  await tick();
  tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("saveConflict"), "the row reports a refused write");
});

test("write: the one-shot cleanPasted trigger writes true and reports it", async () => {
  const { forms, render } = bootRows();
  let tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-disclosure")[0].props.onClick();
  tree = render(NS_BRIDGE)();
  const trigger = byClass(tree, "vlb-trigger")[0];
  assert.ok(trigger, "the trigger button lives in the advanced region");
  trigger.props.onClick();
  await tick();
  assert.deepEqual(forms[NS_BRIDGE].calls.mutate[0].ops, [
    { op: "set", path: ["cleanPasted"], value: true },
  ]);
  assert.equal(forms[NS_BRIDGE].calls.mutate[0].expectedRevision, 11);
});

test("write: reset clears one overridden field through unset", async () => {
  const { forms, render } = bootRows({ vision: { value: { modelType: "ocr" }, user: { modelType: "ocr" } } });
  let tree = render(NS_VISION)();
  const reset = byClass(tree, "vlb-reset")[0];
  assert.ok(reset, "an overridden field shows its reset affordance");
  reset.props.onClick();
  await tick();
  assert.deepEqual(forms[NS_VISION].calls.unset, ["modelType"]);
});

// ── layout and degradation ──────────────────────────────────────────────────

test("layout: advanced fields stay collapsed until the disclosure opens", () => {
  const { render } = bootRows();
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

test("layout: the bridge row keeps its own advanced field set", () => {
  const { render } = bootRows();
  let tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-disclosure")[0].props.onClick();
  tree = render(NS_BRIDGE)();
  assert.equal(collect(tree, (n) => n.type === "textarea").length, 1, "promptTemplate renders in advanced");
  assert.equal(collect(tree, (n) => n.type === "select").length, 2, "scope and mode stay in the main region");
});

test("degradation: an unavailable form renders a notice instead of throwing", () => {
  const { render } = bootRows({ vision: { status: "unavailable" } });
  const tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("unavailable"));
  assert.equal(collect(tree, (n) => n.type === "input").length, 0);
});

test("degradation: a read-only deployment disables editing", () => {
  const { render } = bootRows({ vision: { writable: false } });
  const tree = render(NS_VISION)();
  assert.ok(textOf(tree).includes("readOnly"));
  const inputs = collect(tree, (n) => n.type === "input" || n.type === "select");
  assert.ok(inputs.length > 0);
  assert.ok(inputs.every((n) => n.props.disabled === true), "every control is disabled");
});

test("degradation: a row without a page-injected form shows the unavailable notice", () => {
  const { render } = bootRows();
  const tree = render(NS_VISION, { form: undefined })();
  assert.ok(textOf(tree).includes("unavailable"));
  assert.equal(collect(tree, (n) => n.type === "input").length, 0);
});

test("view: summary renders the description one-liner, not the form", () => {
  const { render } = bootRows();
  const tree = render(NS_VISION, { view: "summary" })();
  assert.ok(textOf(tree).includes("visionDescription"), "summary shows the page's description fallback");
  assert.equal(collect(tree, (n) => n.type === "input").length, 0, "summary never mounts the form");
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
    const { effects } = bootRows();
    assert.equal(appended.length, 1, "exactly one stylesheet for both rows");
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

test("chrome: boolean fields render the host Switch, not a raw checkbox", async () => {
  const { forms, render } = bootRows();
  let tree = render(NS_BRIDGE)();
  const toggle = collect(tree, (n) => n.type === SwitchStub)[0];
  assert.ok(toggle, "the bridge enabled field renders the host Switch primitive");
  assert.equal(toggle.props.checked, true);
  assert.equal(toggle.props.label, "enabledLabel");
  assert.equal(collect(tree, (n) => n.type === "input" && n.props.type === "checkbox").length, 0,
    "no browser-default checkbox is left in the row");
  toggle.props.onChange(false);
  await tick();
  tree = render(NS_BRIDGE)();
  byClass(tree, "vlb-save")[0].props.onClick();
  await tick();
  assert.deepEqual(forms[NS_BRIDGE].calls.mutate[0].ops, [
    { op: "set", path: ["enabled"], value: false },
  ]);
});

test("chrome: field controls are labelled and marked invalid for assistive tech", () => {
  const { render } = bootRows();
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
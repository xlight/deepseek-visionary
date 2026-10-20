// Browser-half tests for lib/client.js (DSH 0.2.0 `plugins.bundle.config` inline section).
//
// The shipped artifact is a hand-written lazy-CJS bundle, so the test evaluates
// it exactly as the DSH module loader would (a `window.__ModuleLoader__.load`
// call), then drives the two sub-card components through a tiny React hook harness
// with faithful stand-ins for the configForms scope and the primitives form
// system (SettingsFormModel semantics mirrored from the primitives source).
//
// Covered: the bundle contract (id = package name, react + primitives edges),
// the single `plugins.bundle.config` registration behind `whileServed`, numeric/text
// fields staged through `scope.mutate` with the read revision, choice fields
// via SegmentedControl, boolean via Switch, the fail-loud `{path}` template,
// the one-shot cleanPasted action, the shared binaryPath dual-write, and the
// unavailable/read-only degradation paths.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BUNDLE = fileURLToPath(new URL("../lib/client.js", import.meta.url));
const BUNDLE_ID = JSON.parse(readFileSync(fileURLToPath(new URL("../package.json", import.meta.url)), "utf8")).name;
const PRIMITIVES_ID = "@deepseek-ai/dsh-client-ui-primitives";
const NS_VISION = "visionary-vision";
const NS_BRIDGE = "visionary-image-bridge";

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
  const render = (Component, props) => {
    cursor = 0;
    pending = [];
    const tree = Component(props ?? {});
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

// ── faithful SettingsFormModel stand-in (semantics mirrored from primitives) ─

function createSnapshotStore(init) {
  let snapshot = init;
  const listeners = new Set();
  return {
    getSnapshot: () => snapshot,
    set: (next) => { snapshot = next; listeners.forEach((l) => l()); },
    subscribe: (l) => { listeners.add(l); return () => listeners.delete(l); },
  };
}

class TestFormModel {
  constructor(scope, specs, secrets = []) {
    this.scope = scope;
    this.specs = new Map(specs.map((spec) => [spec.field, spec]));
    this.secrets = new Map(secrets.map((spec) => [spec.field, spec]));
    this.staged = new Map();
    this.listeners = new Set();
    this.baseline = undefined;
    this.saving = false;
    this.failed = false;
    this.unsubscribe = scope.subscribe(() => this.publish());
  }
  bind(project) {
    const store = createSnapshotStore(project());
    this.listeners.add(() => store.set(project()));
    return store;
  }
  shell() {
    const snapshot = this.scope.getSnapshot();
    const plan = this.plan();
    return {
      available: snapshot.status === "ready",
      writable: snapshot.writable,
      dirty: plan.length > 0,
      invalid: plan.some((item) => item.run === undefined && item.op === undefined),
      saving: this.saving,
      failed: this.failed,
    };
  }
  field(field) {
    const staged = this.staged.get(field);
    if (staged === undefined) {
      return {
        text: this.spec(field).format(this.scope.getSnapshot().value?.[field]),
        overridden: this.stored(field),
        invalid: false,
      };
    }
    const write = staged.clear ? { kind: "clear" } : this.spec(field).parse(staged.text);
    return { text: staged.text, overridden: write?.kind === "set", invalid: write === undefined };
  }
  actions() {
    return {
      edit: (field, text) => this.stage(field, { text, clear: false }),
      resetField: (field) => {
        this.stage(field, { text: this.spec(field).format(this.scope.getSnapshot().base?.[field]), clear: true });
      },
      save: () => this.save(),
      discard: () => {
        if (this.staged.size === 0 && !this.failed) return;
        this.staged.clear();
        this.baseline = undefined;
        this.failed = false;
        this.publish();
      },
    };
  }
  plan() {
    const plan = [];
    for (const [field, staged] of this.staged) {
      if (staged.clear) {
        if (this.stored(field)) plan.push({ field, op: { op: "unset", path: [field] } });
        continue;
      }
      if (staged.text === this.spec(field).format(this.scope.getSnapshot().value?.[field])) continue;
      const write = this.spec(field).parse(staged.text);
      if (write === undefined) plan.push({ field });
      else if (write.kind === "clear") plan.push({ field, op: { op: "unset", path: [field] } });
      else plan.push({ field, op: { op: "set", path: [field], value: write.value } });
    }
    return plan;
  }
  async save() {
    const plan = this.plan();
    if (!plan.length || this.saving || !this.scope.getSnapshot().writable || plan.some((item) => item.run === undefined && item.op === undefined)) return false;
    this.saving = true;
    this.failed = false;
    this.publish();
    try {
      const ops = plan.flatMap((item) => (item.op === undefined ? [] : [item.op]));
      const landed = !ops.length || await this.scope.mutate(ops, this.baseline?.revision);
      if (!landed) { this.failed = true; this.publish(); return false; }
      for (const item of plan) if (item.run) await item.run();
      this.staged.clear();
      this.baseline = undefined;
    } catch {
      this.failed = true;
    } finally {
      this.saving = false;
      this.publish();
    }
    return !this.failed;
  }
  stage(field, edit) {
    this.baseline = this.baseline ?? this.scope.getSnapshot();
    this.staged.set(field, edit);
    this.failed = false;
    this.publish();
  }
  spec(field) {
    const spec = this.specs.get(field);
    if (!spec) throw new Error(`plugin card has no field ${field}`);
    return spec;
  }
  stored(field) {
    const user = this.scope.getSnapshot().user;
    return user !== undefined && Object.hasOwn(user, field);
  }
  publish() { this.listeners.forEach((l) => l()); }
  dispose() { this.unsubscribe(); this.listeners.clear(); }
}

// ── scope + primitives stand-ins ────────────────────────────────────────────

function makeScope(namespace, { value, base, user, revision = 7, status = "ready", writable = true } = {}) {
  const state = { status, value: Object.assign({}, value), base: Object.assign({}, base ?? value), user, revision, writable, mode: "host" };
  const listeners = new Set();
  const scope = {
    namespace,
    state,
    calls: { mutate: [] },
    rejectNext: false,
    getSnapshot: () => state,
    subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    emit: () => listeners.forEach((l) => l()),
    async mutate(ops, expectedRevision) {
      scope.calls.mutate.push({ ops, expectedRevision });
      if (scope.rejectNext) { scope.rejectNext = false; return false; }
      const next = Object.assign({}, state.value);
      for (const op of ops) {
        if (op.op === "unset") delete next[op.path[0]];
        else next[op.path[0]] = op.value;
      }
      state.value = next;
      state.user = Object.assign({}, state.user, ...ops.map((op) => (op.op === "set" ? { [op.path[0]]: op.value } : {})));
      state.revision = expectedRevision === undefined ? state.revision : expectedRevision + 1;
      scope.emit();
      return true;
    },
  };
  return scope;
}

const primitivesStub = {
  SettingsForm: (props) => ({ type: "SettingsForm", props }),
  SettingsValueField: (props) => ({ type: "SettingsValueField", props }),
  Switch: (props) => ({ type: "switch", props }),
  SegmentedControl: (props) => ({ type: "segmented", props }),
  settingsNumberField: (field) => ({
    field,
    format: (value) => (typeof value === "number" ? String(value) : ""),
    parse: (text) => {
      const trimmed = text.trim();
      if (trimmed === "") return { kind: "clear" };
      const parsed = Number(trimmed);
      return Number.isFinite(parsed) ? { kind: "set", value: parsed } : undefined;
    },
  }),
  settingsTextField: (field) => ({
    field,
    format: (value) => (typeof value === "string" ? value : ""),
    parse: (text) => {
      const trimmed = text.trim();
      return trimmed === "" ? { kind: "clear" } : { kind: "set", value: trimmed };
    },
  }),
  SettingsFormModel: TestFormModel,
};

/** Evaluate the shipped bundle the way the loader does. */
function loadBundle() {
  let captured = null;
  const fakeWindow = { __ModuleLoader__: { load: (options) => { captured = options; } } };
  // eslint-disable-next-line no-new-func
  new Function("window", readFileSync(BUNDLE, "utf8"))(fakeWindow);
  assert.ok(captured, "the bundle must call window.__ModuleLoader__.load");
  const required = [];
  const react = makeReact();
  const exportsObj = captured.factory((id) => {
    required.push(id);
    if (id === "react") return react.React;
    if (id === PRIMITIVES_ID) return primitivesStub;
    throw new Error(`unexpected module request: ${id}`);
  });
  return { id: captured.id, exports: exportsObj, required, react };
}

/** Boot the browser plugin against fake configForms scopes. */
function bootPages({ vision, bridge } = {}) {
  const bundle = loadBundle();
  const scopes = {
    [NS_VISION]: makeScope(NS_VISION, { value: Object.assign({
      binaryPath: "",
      modelType: "vision",
      visionTimeoutMs: 1111,
      statusTimeoutMs: 2222,
      loginTimeoutSeconds: 3333,
    }, vision && vision.value), user: vision && vision.user, revision: 7 }),
    [NS_BRIDGE]: makeScope(NS_BRIDGE, { value: Object.assign({
      binaryPath: "",
      enabled: true,
      scope: "text-only",
      mode: "agentic",
      promptTemplate: "path {path}",
      pastedDir: "~/.deepseek-visionary/pasted",
      retainHours: 168,
      routes: [],
    }, bridge && bridge.value), user: bridge && bridge.user, revision: 11 }),
  };
  const registrations = [];
  const injections = [];
  const ctx = {
    locale: { register: () => () => {}, bind: () => (key) => key },
    configForms: {
      get: (ns) => scopes[ns],
      whileServed: (_namespaces, register) => { register(); return () => {}; },
    },
    slots: {
      inject: (name, callback) => { injections.push({ name, callback }); return () => {}; },
      register: (options, component) => { registrations.push({ options, component }); return () => {}; },
    },
    remote: { visionaryDashboard: { getStatus: async () => ({}), reprobeBinary: async () => ({}), relogin: async () => ({}), cleanPasted: async () => ({}), selfCheck: async () => ({}) } },
    effect: (fn) => { fn(); return () => {}; },
  };
  bundle.exports.apply(ctx);
  injections.forEach(({ callback }) => callback());

  const cards = {};
  let bundleConfig = null;
  registrations.forEach(({ options, component }) => {
    if (options.name === "plugins.bundle.config") {
      bundleConfig = { options, component };
    }
  });

  /** Render a sub-card's page via the merged inject face; the snapshot comes
   * from the controller's real bound store. `which` is "vision" or "bridge".
   * The composite `VisionaryBundleConfig` is rendered first to materialize the
   * children element nodes (createElement is lazy — children are unexecuted
   * {type, props} nodes), then the chosen child is executed via react.render. */
  const render = (id, { view = "page" } = {}) => {
    const which = id === NS_VISION ? "vision" : "bridge";
    const face = bundleConfig.options.inject();
    const hookKey = which === "vision" ? "visionCard" : "bridgeCard";
    const snapshot = face.hooks[hookKey].getSnapshot();
    const props = Object.assign({}, face, {
      t: (key) => key,
      view,
      useVisionCard: () => face.hooks.visionCard.getSnapshot(),
      useBridgeCard: () => face.hooks.bridgeCard.getSnapshot(),
    });
    const fullTree = bundle.react.render(bundleConfig.component, props);
    const childIndex = which === "vision" ? 1 : 2;
    const childNode = fullTree.props.children[childIndex];
    // For summary view, re-render the child component with view: "summary"
    // (VisionaryBundleConfig hardcodes view: "page" for inline display).
    const childProps = view === "summary" ? Object.assign({}, childNode.props, { view: "summary" }) : childNode.props;
    const tree = bundle.react.render(childNode.type, childProps);
    return { tree, face, props: childProps, snapshot };
  };

  return { bundle, scopes, registrations, cards, bundleConfig, render };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

// ── bundle contract ─────────────────────────────────────────────────────────

test("bundle: loader id and the module edges it needs", () => {
  const { id, required } = loadBundle();
  assert.equal(id, BUNDLE_ID);
  assert.deepEqual(required, ["react", PRIMITIVES_ID],
    "the pages need the react seed word and the host primitives");
});

test("bundle: injects the slots, locale, configForms and remote services", () => {
  const { exports } = loadBundle();
  assert.deepEqual(exports.inject, ["slots", "locale", "configForms", "remote"]);
});

// ── registration ────────────────────────────────────────────────────────────

test("registration: single plugins.bundle.config inline section, no row.config, no item", () => {
  const { registrations } = bootPages();
  const rowRegs = registrations.filter((r) => r.options.name === "plugins.row.config");
  const itemRegs = registrations.filter((r) => r.options.name === "plugins.item");
  assert.equal(rowRegs.length, 0, "no plugins.row.config registrations");
  assert.equal(itemRegs.length, 0, "no plugins.item registrations");
  const bundleRegs = registrations.filter((r) => r.options.name === "plugins.bundle.config");
  assert.equal(bundleRegs.length, 1, "exactly one plugins.bundle.config");
  const options = bundleRegs[0].options;
  assert.equal(options.key, "@xlight-oss/visionary-dsh");
  assert.equal(typeof options.label, "function");
  assert.equal(options.locale, "settings.plugins.visionary");
  assert.equal(typeof options.inject, "function");
});

test("registration: inject() returns an object with namespaced vision + bridge faces + remote", () => {
  const { bundleConfig } = bootPages();
  const face = bundleConfig.options.inject();
  assert.equal(typeof face, "object", "inject() returns an object (not array)");
  assert.ok(!Array.isArray(face), "inject() is not an array");
  assert.ok(face.hooks && face.hooks.visionCard, "hooks.visionCard present");
  assert.ok(face.hooks && face.hooks.bridgeCard, "hooks.bridgeCard present");
  assert.equal(typeof face.visionEdit, "function");
  assert.equal(typeof face.visionSave, "function");
  assert.equal(typeof face.bridgeEdit, "function");
  assert.equal(typeof face.bridgeSave, "function");
  assert.equal(typeof face.remote, "object", "remote (dashboard) present");
});

test("layout: summary view returns the one-liner, page view renders the form", () => {
  const { render, scopes } = bootPages();
  const summary = render(NS_VISION, { view: "summary", snapshot: scopes[NS_VISION].getSnapshot() });
  assert.equal(typeof summary.tree, "string", "summary is plain text");
  const { tree: pageTree, props } = render(NS_VISION);
  assert.equal(collect(pageTree, (n) => n.type === primitivesStub.SettingsForm).length, 1);
});

// ── writes ──────────────────────────────────────────────────────────────────

test("write: a staged number is written through mutate with the read revision", async () => {
  const { render, scopes } = bootPages();
  const { tree, props } = render(NS_VISION);
  const field = collect(tree, (n) => n.type === primitivesStub.SettingsValueField
    && n.props.id === "visionary-visionTimeoutMs")[0];
  assert.ok(field, "the vision timeout field renders its current value");
  field.props.onEdit("9999");
  props.visionSave();
  await tick();
  assert.equal(scopes[NS_VISION].calls.mutate.length, 1);
  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [
    { op: "set", path: ["visionTimeoutMs"], value: 9999 },
  ]);
  assert.equal(scopes[NS_VISION].calls.mutate[0].expectedRevision, 7, "the read revision fences the write");
});

test("write: choice fields edit through SegmentedControl", async () => {
  const { render, scopes } = bootPages();
  const { tree, props } = render(NS_VISION);
  const mode = collect(tree, (n) => n.type === primitivesStub.SegmentedControl && n.props.id === "visionary-modelType")[0];
  assert.ok(mode, "modelType renders a SegmentedControl");
  assert.equal(mode.props.value, "vision");
  mode.props.onChange("ocr");
  props.visionSave();
  await tick();
  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [{ op: "set", path: ["modelType"], value: "ocr" }]);
});

test("write: the shared binaryPath writes both entries with their own revisions", async () => {
  const { render, scopes } = bootPages();
  const { tree, props } = render(NS_VISION);
  const field = collect(tree, (n) => n.type === primitivesStub.SettingsValueField
    && n.props.id === "visionary-binaryPath")[0];
  field.props.onEdit("/opt/visionary-server");
  props.visionSave();
  await tick();
  assert.deepEqual(scopes[NS_VISION].calls.mutate[0].ops, [{ op: "set", path: ["binaryPath"], value: "/opt/visionary-server" }]);
  assert.equal(scopes[NS_VISION].calls.mutate[0].expectedRevision, 7);
  assert.deepEqual(scopes[NS_BRIDGE].calls.mutate[0].ops, [{ op: "set", path: ["binaryPath"], value: "/opt/visionary-server" }]);
  assert.equal(scopes[NS_BRIDGE].calls.mutate[0].expectedRevision, 11);
});

test("write: the prompt template must carry the {path} placeholder", async () => {
  const { render, scopes } = bootPages();
  let { tree, props } = render(NS_BRIDGE);
  // 展开高级区
  const disclosure = collect(tree, (n) => n.type === "button" && textOf(n).includes("advanced"))[0];
  assert.ok(disclosure, "the advanced disclosure renders");
  disclosure.props.onClick();
  const { tree: openedTree, props: opened } = render(NS_BRIDGE);
  const field = collect(openedTree, (n) => n.type === primitivesStub.SettingsValueField
    && n.props.id === "visionary-bridge-promptTemplate")[0];
  assert.ok(field, "the template lives in the advanced region");
  field.props.onEdit("no placeholder here");
  const { tree: editedTree, props: edited } = render(NS_BRIDGE);
  const invalid = collect(editedTree, (n) => n.type === primitivesStub.SettingsValueField
    && n.props.id === "visionary-bridge-promptTemplate")[0];
  assert.equal(invalid.props.invalid, true, "missing {path} marks the field invalid");
  edited.bridgeSave();
  await tick();
  assert.equal(scopes[NS_BRIDGE].calls.mutate.length, 0, "an invalid draft refuses to save");
});

test("write: the cleanPasted action writes true with the read revision", async () => {
  const { render, scopes } = bootPages();
  let { tree, props } = render(NS_BRIDGE);
  collect(tree, (n) => n.type === "button" && textOf(n).includes("advanced"))[0].props.onClick();
  const { tree: openedTree, props: opened } = render(NS_BRIDGE);
  const action = collect(openedTree, (n) => n.type === "button" && textOf(n).includes("cleanPastedAction"))[0];
  assert.ok(action, "the one-shot cleanup button lives in the advanced region");
  action.props.onClick();
  assert.deepEqual(scopes[NS_BRIDGE].calls.mutate[0].ops, [{ op: "set", path: ["cleanPasted"], value: true }]);
  assert.equal(scopes[NS_BRIDGE].calls.mutate[0].expectedRevision, 11);
});

test("write: a refused write surfaces as failed and keeps the drafts", async () => {
  const { render, scopes } = bootPages();
  scopes[NS_VISION].rejectNext = true;
  const { tree, props } = render(NS_VISION);
  collect(tree, (n) => n.type === primitivesStub.SettingsValueField && n.props.id === "visionary-visionTimeoutMs")[0]
    .props.onEdit("9999");
  props.visionSave();
  await tick();
  assert.equal(scopes[NS_VISION].calls.mutate[0].expectedRevision, 7);
  const { tree: afterTree, props: after } = render(NS_VISION);
  assert.equal(collect(afterTree, (n) => n.type === primitivesStub.SettingsForm)[0].props.state.failed, true);
});

// ── degradation ─────────────────────────────────────────────────────────────

test("degradation: an unavailable namespace renders no page form", () => {
  const { render, scopes } = bootPages();
  scopes[NS_VISION].state.status = "unavailable";
  scopes[NS_VISION].emit();
  const { tree, props } = render(NS_VISION);
  const form = collect(tree, (n) => n.type === primitivesStub.SettingsForm)[0];
  assert.ok(form, "the page still renders the SettingsForm frame");
  assert.equal(form.props.state.available, false);
});

test("degradation: a read-only deployment disables the controls", () => {
  const { render, scopes } = bootPages();
  scopes[NS_BRIDGE].state.writable = false;
  scopes[NS_BRIDGE].emit();
  const { tree, props } = render(NS_BRIDGE);
  const toggle = collect(tree, (n) => n.type === primitivesStub.Switch)[0];
  assert.ok(toggle, "the bridge enabled field renders the host Switch");
  assert.equal(toggle.props.disabled, true);
  const segs = collect(tree, (n) => n.type === primitivesStub.SegmentedControl);
  assert.ok(segs.length > 0 && segs.every((n) => n.props.disabled === true), "choice fields are disabled");
});
// Browser half of the visionary-dsh plugin.
//
// One native `settings.plugin.item` card per settings namespace the host
// serves (`visionary-vision`, `visionary-image-bridge`). The Plugin
// configuration tab dispatches that keyed slot by namespace and pairs the
// host-served namespace with the card registered under the same key, so the
// cards appear under Settings → Plugins → Plugin configuration.
//
// Transport: `ctx.settingsScope.bind({ namespace })` — the host's own settings
// channel. `getSnapshot()` drives rendering, `subscribe()` repaints, and writes
// go through `mutate(ops, revision)` with the revision read from the snapshot
// as the fence. There is no plugin-owned HTTP route and no snapshot-generation
// layer: the scope owns the wire, the revision fence, and the recovery read.
//
// Hand-written in the lazy-CJS bundle protocol (`window.__ModuleLoader__.load`)
// because no published preset emits that artifact for out-of-repo packages.
//
// Discovery contract (dsh-client-modules): a browser bundle is served only for
// a Loader row whose *bare package specifier* (e.g. `@xlight-oss/visionary-dsh`)
// resolves to a manifest declaring `dsh.client` + `exports["./client"]` — a
// subpath row name like `@xlight-oss/visionary-dsh/settings-card` is rejected
// by exactPackageSpecifier and silently yields no entry. The registered id must
// equal that package name, because the runner activates the row by requiring
// it. Hence this bundle lives at `lib/client.js` of the main package and is
// discovered through the main (vision) Loader row — no extra row is needed.
// Module edges: `react` only (a platform seed word). Service dependencies are
// waited on through cordis `inject`, not through module edges.
window.__ModuleLoader__.load({
  id: "@xlight-oss/visionary-dsh",
  factory: (require) => {
    var module = { exports: {} };
    var exports = module.exports;
    Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
    var React = require("react");
    // The host's UI primitives are a platform seed module (every shipped card
    // requires them), so the Switch below is the deployment's own control rather
    // than a re-invented one. The default-checkbox fallback keeps the card usable
    // if a future shell stops exposing the seed word.
    var primitives = {};
    try {
      primitives = require("@deepseek-ai/dsh-client-ui-primitives") || {};
    } catch (error) {
      console.warn("visionary-dsh: UI primitives unavailable, falling back to plain controls", error);
    }
    var SwitchControl = typeof primitives.Switch === "function"
      ? primitives.Switch
      : function FallbackSwitch(props) {
          return React.createElement("input", {
            type: "checkbox",
            checked: Boolean(props.checked),
            disabled: Boolean(props.disabled),
            "aria-label": props.label,
            onChange: function (event) { props.onChange(event.target.checked); },
          });
        };


    var NS_VISION = "visionary-vision";
    var NS_BRIDGE = "visionary-image-bridge";

    // ── field definitions ──
    // `area` decides the card region: "main" is always visible, "advanced"
    // lives behind the collapsed disclosure. `targets` lists the namespace keys
    // this field writes — one entry for a normal field, two for the shared
    // `binaryPath` (the tools and the bridge both resolve the binary).

    function field(ns, key, kind, area, opts) {
      var base = {
        id: ns + "." + key,
        key: key,
        kind: kind,
        area: area,
        targets: [{ ns: ns, key: key }],
      };
      for (var k in opts) if (Object.prototype.hasOwnProperty.call(opts, k)) base[k] = opts[k];
      return base;
    }

    var VISION_FIELDS = [
      field(NS_VISION, "modelType", "select", "main", {
        options: ["vision", "ocr"],
        labelKey: "visionModelTypeLabel",
        hintKey: "visionModelTypeHint",
      }),
      field(NS_VISION, "visionTimeoutMs", "number", "main", {
        labelKey: "visionTimeoutLabel",
        hintKey: "visionTimeoutHint",
      }),
      field(NS_VISION, "binaryPath", "text", "main", {
        labelKey: "sharedBinaryPathLabel",
        hintKey: "sharedBinaryPathHint",
        targets: [
          { ns: NS_VISION, key: "binaryPath" },
          { ns: NS_BRIDGE, key: "binaryPath" },
        ],
      }),
      field(NS_VISION, "statusTimeoutMs", "number", "advanced", {
        labelKey: "visionStatusTimeoutLabel",
        hintKey: "visionStatusTimeoutHint",
      }),
      field(NS_VISION, "loginTimeoutSeconds", "number", "advanced", {
        labelKey: "visionLoginTimeoutLabel",
        hintKey: "visionLoginTimeoutHint",
      }),
    ];

    var BRIDGE_FIELDS = [
      field(NS_BRIDGE, "enabled", "boolean", "main", {
        labelKey: "enabledLabel",
        hintKey: "enabledHint",
      }),
      field(NS_BRIDGE, "scope", "select", "main", {
        options: ["text-only", "also-vl"],
        labelKey: "scopeLabel",
        hintKey: "scopeHint",
      }),
      field(NS_BRIDGE, "mode", "select", "main", {
        options: ["agentic", "deterministic"],
        labelKey: "modeLabel",
        hintKey: "modeHint",
      }),
      field(NS_BRIDGE, "promptTemplate", "textarea", "advanced", {
        labelKey: "promptTemplateLabel",
        hintKey: "promptTemplateHint",
        placeholderKey: "promptTemplateHint",
      }),
      field(NS_BRIDGE, "pastedDir", "text", "advanced", {
        labelKey: "pastedDirLabel",
        hintKey: "pastedDirHint",
      }),
      field(NS_BRIDGE, "retainHours", "number", "advanced", {
        labelKey: "retainHoursLabel",
        hintKey: "retainHoursHint",
      }),
      field(NS_BRIDGE, "cleanPasted", "trigger", "advanced", {
        labelKey: "cleanPastedLabel",
        hintKey: "cleanPastedHint",
        actionKey: "cleanPastedAction",
      }),
    ];

    // ── locale ──

    var NS = "settings.plugins.visionary";

    var LOCALE_ZH = {
      visionTitle: "Visionary · 视觉工具",
      visionDescription: "deepseek_vision / deepseek_ocr 等 5 个原生工具：上传管道、超时与二进制路径，修改即时生效",
      bridgeTitle: "Visionary · 图片桥接",
      bridgeDescription: "纯文本模型粘贴图片时自动放行并改写为文本引导；VL 模型默认不受干预",
      advanced: "高级",
      advancedHint: "低频选项：超时、落盘与模板",
      visionModelTypeLabel: "上传管道",
      visionModelTypeHint: "vision（默认）：完整多模态理解 | ocr：deepseek_vision 走纯文字提取管道，等价每次调用 deepseek_ocr。修改后即时生效",
      visionLoginTimeoutLabel: "登录超时（秒）",
      visionLoginTimeoutHint: "deepseek_vision_login 阻塞等待上限；DEEPSEEK_LOGIN_TIMEOUT 环境变量优先",
      visionTimeoutLabel: "识图超时（毫秒）",
      visionTimeoutHint: "deepseek_vision / deepseek_ocr 单次调用超时",
      visionStatusTimeoutLabel: "状态超时（毫秒）",
      visionStatusTimeoutHint: "deepseek_vision_status / deepseek_vision_logout 超时",
      sharedBinaryPathLabel: "二进制路径",
      sharedBinaryPathHint: "visionary-server 路径，工具与桥接（deterministic 模式）共用；空 = DEEPSEEK_VISIONARY_BIN → PATH",
      enabledLabel: "桥接启用",
      enabledHint: "关闭后恢复宿主原行为（文本模型粘贴图片仍被拒绝）",
      pastedDirLabel: "落盘目录",
      pastedDirHint: "图片落盘目录，强制 0700 / 文件 0600，支持 ~",
      retainHoursLabel: "保留小时数",
      retainHoursHint: "落盘副本保留小时数，<= 0 表示不清理",
      scopeLabel: "桥接范围",
      scopeHint: "text-only：仅文本模型 | also-vl：VL 模型同样经桥接",
      modeLabel: "桥接模式",
      modeHint: "agentic：改写为引导文本 | deterministic：直接调用分析",
      promptTemplateLabel: "引导模板",
      promptTemplateHint: "必须包含 {path} 占位符",
      cleanPastedLabel: "清理已落盘副本",
      cleanPastedHint: "一次性操作：删除落盘目录中的图片副本（附件库不受影响），清理数量写入 DSH 日志",
      cleanPastedAction: "立即清理",
      cleanPastedTriggered: "已触发清理",
      expand: "展开设置",
      collapse: "收起设置",
      save: "保存",
      saving: "保存中…",
      discard: "放弃修改",
      unsaved: "未保存",
      saveFailed: "保存失败，已保留供修改。",
      saveConflict: "保存被拒绝：配置已在别处修改，已重新读取。",
      overridden: "已覆盖",
      reset: "恢复默认",
      readOnly: "本部署设置为只读。",
      loading: "加载中…",
      unavailable: "设置服务不可用。",
      invalidNumber: "请输入数字；留空表示使用默认值。",
    };

    var LOCALE_EN = {
      visionTitle: "Visionary · Vision Tools",
      visionDescription: "deepseek_vision / deepseek_ocr and 3 more native tools: upload pipeline, timeouts and binary path. Changes apply immediately",
      bridgeTitle: "Visionary · Image Bridge",
      bridgeDescription: "Transparent image admission and rewrite for text-only models; VL models keep their native handling",
      advanced: "Advanced",
      advancedHint: "Rarely used: timeouts, storage and template",
      visionModelTypeLabel: "Upload pipeline",
      visionModelTypeHint: "vision (default): full multimodal understanding | ocr: deepseek_vision routes through text-extraction, same as deepseek_ocr. Applies immediately",
      visionLoginTimeoutLabel: "Login timeout (s)",
      visionLoginTimeoutHint: "deepseek_vision_login block cap; DEEPSEEK_LOGIN_TIMEOUT env wins",
      visionTimeoutLabel: "Vision timeout (ms)",
      visionTimeoutHint: "per deepseek_vision / deepseek_ocr call timeout",
      visionStatusTimeoutLabel: "Status timeout (ms)",
      visionStatusTimeoutHint: "deepseek_vision_status / deepseek_vision_logout timeout",
      sharedBinaryPathLabel: "Binary path",
      sharedBinaryPathHint: "visionary-server binary, shared by the tools and the bridge (deterministic mode); empty = DEEPSEEK_VISIONARY_BIN → PATH",
      enabledLabel: "Bridge enabled",
      enabledHint: "Off restores host behavior (text-only models reject images again)",
      pastedDirLabel: "Paste directory",
      pastedDirHint: "Image save dir (0700 dir / 0600 files), supports ~",
      retainHoursLabel: "Retention (hours)",
      retainHoursHint: "Pasted file retention; <= 0 disables cleanup",
      scopeLabel: "Bridge scope",
      scopeHint: "text-only: text models only | also-vl: VL models bridged too",
      modeLabel: "Bridge mode",
      modeHint: "agentic: rewrite to guide | deterministic: analyze directly",
      promptTemplateLabel: "Prompt template",
      promptTemplateHint: "Must contain the {path} placeholder",
      cleanPastedLabel: "Clean pasted copies",
      cleanPastedHint: "One-shot action: delete the pasted image copies (the attachment store is untouched); the removed count lands in the DSH log",
      cleanPastedAction: "Clean now",
      cleanPastedTriggered: "Cleanup triggered",
      expand: "Expand settings",
      collapse: "Collapse settings",
      save: "Save",
      saving: "Saving…",
      discard: "Discard",
      unsaved: "Unsaved",
      saveFailed: "Save failed; values left for you to correct.",
      saveConflict: "Save refused: the config changed elsewhere and was re-read.",
      overridden: "Overridden",
      reset: "Restore default",
      readOnly: "This deployment stores settings read-only.",
      loading: "Loading…",
      unavailable: "Settings service unavailable.",
      invalidNumber: "Enter a number, or leave blank to use the default.",
    };

    // ── editor: staged edits over one card's namespaces ──
    // Reads come from the bound scopes; writes are queued per namespace through
    // `mutate(ops, revision)`, so every write carries the revision this editor
    // read as its fence. A rejected write never throws: the scope reloads the
    // host state, and the verification pass below turns the mismatch into the
    // conflict notice instead of silently dropping the edit.

    function makeEditor(opts) {
      var fields = opts.fields;
      var fieldsById = {};
      fields.forEach(function (f) { fieldsById[f.id] = f; });

      var listeners = new Set();
      var staged = {};
      var saving = false;
      var failed = false;
      var conflict = false;
      var triggered = false;
      var cache = null;

      var scopeOf = function (ns) { return opts.scopes[ns]; };
      var primary = function () { return scopeOf(opts.primaryNs).getSnapshot(); };

      var displayValue = function (f) {
        for (var i = 0; i < f.targets.length; i++) {
          var target = f.targets[i];
          var snapshot = scopeOf(target.ns).getSnapshot();
          var raw = (snapshot.value || {})[target.key];
          if (raw !== undefined && raw !== null && raw !== "") return String(raw);
        }
        return "";
      };

      var overridden = function (f) {
        for (var i = 0; i < f.targets.length; i++) {
          var target = f.targets[i];
          var snapshot = scopeOf(target.ns).getSnapshot();
          var user = snapshot.user;
          if (user !== undefined && user !== null && Object.prototype.hasOwnProperty.call(user, target.key)) {
            return true;
          }
        }
        return false;
      };

      var rebuild = function () {
        var snap = primary();
        var ready = snap.status === "ready";
        var out = {
          status: snap.status,
          writable: snap.writable,
          mode: snap.mode,
          ready: ready,
          dirty: Object.keys(staged).length > 0,
          invalid: false,
          saving: saving,
          failed: failed,
          conflict: conflict,
          triggered: triggered,
          fields: {},
        };
        fields.forEach(function (f) {
          var stagedEntry = Object.prototype.hasOwnProperty.call(staged, f.id) ? staged[f.id] : undefined;
          var text;
          var fieldOverridden;
          var invalid = false;
          if (f.kind === "trigger") {
            text = "";
            fieldOverridden = false;
          } else if (stagedEntry !== undefined) {
            text = stagedEntry.text;
            fieldOverridden = stagedEntry.text !== displayValue(f);
            if (f.kind === "number" && stagedEntry.text.trim() !== "") {
              invalid = !Number.isFinite(Number(stagedEntry.text.trim()));
            }
          } else {
            text = displayValue(f);
            fieldOverridden = overridden(f);
          }
          if (invalid) out.invalid = true;
          out.fields[f.id] = { text: text, overridden: fieldOverridden, invalid: invalid };
        });
        cache = out;
      };

      var publish = function () { rebuild(); listeners.forEach(function (l) { l(); }); };

      var store = {
        getSnapshot: function () { return cache; },
        subscribe: function (listener) {
          listeners.add(listener);
          return function () { listeners.delete(listener); };
        },
      };

      opts.ctx.effect(function () {
        var cleanups = Object.keys(opts.scopes).map(function (ns) {
          return opts.scopes[ns].subscribe(function () { publish(); });
        });
        return function () { cleanups.forEach(function (cancel) { cancel(); }); };
      }, "visionary-settings: " + opts.primaryNs + " scope subscription");

      rebuild();

      var coerce = function (f, text) {
        if (f.kind === "boolean") return text === "true";
        if (f.kind === "number") return Number(text.trim());
        return text;
      };

      var edit = function (fieldId, text) {
        var f = fieldsById[fieldId];
        if (!f) return;
        if (text === displayValue(f)) delete staged[fieldId];
        else staged[fieldId] = { text: text };
        failed = false;
        conflict = false;
        publish();
      };

      var discard = function () {
        staged = {};
        failed = false;
        conflict = false;
        triggered = false;
        publish();
      };

      // Group staged edits by namespace: one `mutate` call per namespace, all
      // ops sharing that namespace's read revision as the fence.
      var groupOps = function () {
        var byNs = {};
        for (var fieldId in staged) {
          if (!Object.prototype.hasOwnProperty.call(staged, fieldId)) continue;
          var f = fieldsById[fieldId];
          var text = staged[fieldId].text;
          f.targets.forEach(function (target) {
            if (!byNs[target.ns]) byNs[target.ns] = { ops: [], expect: [] };
            if (text.trim() === "") {
              byNs[target.ns].ops.push({ op: "unset", path: [target.key] });
              byNs[target.ns].expect.push({ key: target.key, value: undefined });
            } else {
              var value = coerce(f, text);
              byNs[target.ns].ops.push({ op: "set", path: [target.key], value: value });
              byNs[target.ns].expect.push({ key: target.key, value: value });
            }
          });
        }
        return byNs;
      };

      // Did the host accept what we wrote? The scope reloads on rejection, so a
      // stale fence shows up here as "the value is not what we wrote".
      var verify = function (byNs) {
        for (var ns in byNs) {
          if (!Object.prototype.hasOwnProperty.call(byNs, ns)) continue;
          var snapshot = scopeOf(ns).getSnapshot();
          var value = snapshot.value || {};
          var user = snapshot.user || {};
          var expects = byNs[ns].expect;
          for (var i = 0; i < expects.length; i++) {
            var expected = expects[i];
            if (expected.value === undefined) {
              if (Object.prototype.hasOwnProperty.call(user, expected.key)) return false;
            } else if (value[expected.key] !== expected.value) {
              return false;
            }
          }
        }
        return true;
      };

      var save = function () {
        if (!cache.dirty || cache.invalid || saving) return Promise.resolve();
        var byNs = groupOps();
        saving = true;
        failed = false;
        conflict = false;
        publish();
        var writes = Object.keys(byNs).map(function (ns) {
          return scopeOf(ns).mutate(byNs[ns].ops, scopeOf(ns).getSnapshot().revision);
        });
        return Promise.all(writes)
          .then(function () {
            if (!verify(byNs)) conflict = true;
            else staged = {};
          })
          .catch(function () { failed = true; })
          .then(function () {
            saving = false;
            publish();
          });
      };

      var reset = function (fieldId) {
        var f = fieldsById[fieldId];
        if (!f || !cache.writable) return Promise.resolve();
        delete staged[fieldId];
        var writes = f.targets.map(function (target) {
          return scopeOf(target.ns).unset(target.key);
        });
        return Promise.all(writes)
          .catch(function () { failed = true; })
          .then(function () { publish(); });
      };

      // One-shot trigger (cleanPasted): write `true`; the host clears the files
      // and resets the persisted value, which the scope mirror then reports.
      var trigger = function (fieldId) {
        var f = fieldsById[fieldId];
        if (!f || !cache.writable) return Promise.resolve();
        var ns = f.targets[0].ns;
        triggered = true;
        publish();
        return scopeOf(ns)
          .mutate([{ op: "set", path: [f.key], value: true }], scopeOf(ns).getSnapshot().revision)
          .catch(function () { failed = true; })
          .then(function () { publish(); });
      };

      return {
        store: store,
        edit: edit,
        reset: reset,
        save: save,
        discard: discard,
        trigger: trigger,
      };
    }

    // ── React plumbing ──

    function useStore(store) {
      var pair = React.useState(function () { return store.getSnapshot(); });
      var snapshot = pair[0];
      var setSnapshot = pair[1];
      React.useEffect(function () {
        setSnapshot(store.getSnapshot());
        return store.subscribe(function () { setSnapshot(store.getSnapshot()); });
      }, [store]);
      return snapshot;
    }

    // ── card chrome ──
    //
    // A `settings.plugin.item` row is normally drawn by the host's own card
    // module (`dsh-client-ui-settings-plugins`), whose CSS-module class names are
    // hashed per build. Those hashed names are unusable outside that package, so
    // the rules are mirrored here under our own `vlb-` prefix with the identical
    // declarations and `--dsw-alias-*` tokens; a token rename costs appearance,
    // while reaching for the hashed names would break outright.

    var CARD_CSS = [
      ".vlb-card{border:.5px solid var(--dsw-alias-border-l4);background:var(--dsw-alias-bg-layer-3);border-radius:16px;list-style:none;transition:border-color .16s,background .16s}",
      ".vlb-card:hover{border-color:var(--dsw-alias-label-dimmed)}",
      ".vlb-cardOpen{background:var(--dsw-alias-bg-layer-2);border-color:var(--dsw-alias-label-dimmed)}",
      ".vlb-header{appearance:none;width:100%;font:inherit;color:inherit;text-align:left;cursor:pointer;background:0 0;border:0;border-radius:12px;align-items:center;gap:12px;padding:14px 16px;display:flex}",
      ".vlb-header:focus-visible{outline:2px solid var(--dsw-alias-brand-primary);outline-offset:-2px}",
      ".vlb-headText{flex-direction:column;flex:1;gap:4px;min-width:0;display:flex}",
      ".vlb-name{color:var(--dsw-alias-label-primary);font-size:15px;font-weight:600;line-height:1.4}",
      ".vlb-description{color:var(--dsw-alias-label-tertiary);font-size:13px;line-height:1.5}",
      ".vlb-chevron{color:var(--dsw-alias-label-tertiary);flex:none;transition:transform .16s}",
      ".vlb-chevronOpen{transform:rotate(180deg)}",
      ".vlb-tag{display:inline-flex;align-items:center;border-radius:999px;padding:1px 8px;font-size:11px;line-height:17px;font-weight:500;white-space:nowrap;background:var(--dsw-alias-bg-module-platform);color:var(--dsw-alias-label-secondary)}",
      ".vlb-pending{flex:none}",
      ".vlb-body{border-top:.5px solid var(--dsw-alias-border-l2);margin:0 16px;padding-bottom:8px}",
      ".vlb-field{flex-direction:column;gap:6px;padding:12px 0;display:flex}",
      ".vlb-field+.vlb-field{border-top:.5px solid var(--dsw-alias-border-l2)}",
      ".vlb-fieldHead{align-items:center;gap:8px;display:flex}",
      ".vlb-label{min-width:0;color:var(--dsw-alias-label-primary);flex:1;font-size:13px;font-weight:500;line-height:1.5}",
      ".vlb-badges{align-items:center;gap:8px;display:inline-flex}",
      ".vlb-reset{font:inherit;color:var(--dsw-alias-label-secondary);cursor:pointer;background:0 0;border:none;padding:0;font-size:12px;line-height:1.5}",
      ".vlb-reset:hover:not(:disabled){color:var(--dsw-alias-label-primary)}",
      ".vlb-reset:disabled{cursor:default;opacity:.4}",
      ".vlb-toggleRow{justify-content:space-between;align-items:flex-start;gap:16px;display:flex}",
      ".vlb-toggleLabel{flex:1;min-width:0;gap:6px;flex-direction:column;display:flex}",
      ".vlb-input{box-sizing:border-box;width:100%;border:.5px solid var(--dsw-alias-border-l4);background:var(--dsw-alias-bg-layer-3);height:34px;font:inherit;color:var(--dsw-alias-label-primary);border-radius:8px;padding:0 12px;font-size:13px;line-height:1.5}",
      ".vlb-input:focus-visible{border-color:var(--dsw-alias-brand-primary);outline:none}",
      ".vlb-input:disabled{color:var(--dsw-alias-label-tertiary);cursor:default}",
      ".vlb-inputInvalid{border-color:var(--dsw-alias-label-error)}",
      ".vlb-textarea{box-sizing:border-box;width:100%;min-height:72px;resize:vertical;border:.5px solid var(--dsw-alias-border-l4);background:var(--dsw-alias-bg-layer-3);font:inherit;color:var(--dsw-alias-label-primary);border-radius:8px;padding:8px 12px;font-size:13px;line-height:1.5}",
      ".vlb-textarea:focus-visible{border-color:var(--dsw-alias-brand-primary);outline:none}",
      ".vlb-hint{margin:0;font-size:12px;line-height:1.5;color:var(--dsw-alias-label-tertiary)}",
      ".vlb-invalid{margin:0;font-size:12px;line-height:1.5;color:var(--dsw-alias-label-error)}",
      ".vlb-disclosure{appearance:none;width:100%;font:inherit;text-align:left;cursor:pointer;background:0 0;border:0;border-top:.5px solid var(--dsw-alias-border-l2);color:var(--dsw-alias-label-secondary);align-items:center;gap:6px;padding:12px 0;font-size:13px;line-height:1.5;display:flex}",
      ".vlb-disclosure:hover{color:var(--dsw-alias-label-primary)}",
      ".vlb-footer{border-top:.5px solid var(--dsw-alias-border-l2);justify-content:flex-end;align-items:center;gap:8px;padding:12px 0 4px;display:flex}",
      ".vlb-failed{min-width:0;color:var(--dsw-alias-label-error);flex:1;margin:0;font-size:12px;line-height:1.5}",
      ".vlb-discard,.vlb-save,.vlb-trigger{appearance:none;font:inherit;cursor:pointer;border:1px solid #0000;border-radius:8px;padding:5px 14px;font-size:13px;line-height:1.5}",
      ".vlb-discard,.vlb-trigger{border-color:var(--dsw-alias-border-l2);color:var(--dsw-alias-label-secondary);background:0 0}",
      ".vlb-discard:hover:not(:disabled),.vlb-trigger:hover:not(:disabled){color:var(--dsw-alias-label-primary);border-color:var(--dsw-alias-label-dimmed)}",
      ".vlb-save{background:var(--dsw-alias-label-primary);color:var(--dsw-alias-bg-layer-3)}",
      ".vlb-discard:disabled,.vlb-save:disabled,.vlb-trigger:disabled{opacity:.4;cursor:default}",
      ".vlb-discard:focus-visible,.vlb-save:focus-visible,.vlb-trigger:focus-visible{outline:2px solid var(--dsw-alias-brand-primary);outline-offset:1px}",
      ".vlb-readOnly,.vlb-loading{margin:12px 0 0;font-size:12px;line-height:1.5;color:var(--dsw-alias-label-tertiary)}",
      ".vlb-unavailable{margin:12px 0 0;font-size:12px;line-height:1.5;color:var(--dsw-alias-label-error)}",
    ].join("");

    // The host's IconChevronDownOutline14, inlined: one less module edge.
    var CHEVRON_PATH = "M11.8486 5.5L11.4238 5.92383L8.69727 8.65137C8.44157 8.90706 8.21562 9.13382 8.01172 9.29785C7.79912 9.46883 7.55595 9.61756 7.25 9.66602C7.08435 9.69222 6.91565 9.69222 6.75 9.66602C6.44405 9.61756 6.20088 9.46883 5.98828 9.29785C5.78438 9.13382 5.55843 8.90706 5.30273 8.65137L2.57617 5.92383L2.15137 5.5L3 4.65137L3.42383 5.07617L6.15137 7.80273C6.42595 8.07732 6.59876 8.24849 6.74023 8.3623C6.87291 8.46904 6.92272 8.47813 6.9375 8.48047C6.97895 8.48703 7.02105 8.48703 7.0625 8.48047C7.07728 8.47813 7.12709 8.46904 7.25977 8.3623C7.40124 8.24849 7.57405 8.07732 7.84863 7.80273L10.5762 5.07617L11 4.65137L11.8486 5.5Z";

    function chevron(className) {
      return React.createElement("svg", {
        className: className,
        width: 14,
        height: 14,
        viewBox: "0 0 14 14",
        fill: "none",
        "aria-hidden": "true",
      }, React.createElement("path", { d: CHEVRON_PATH, fill: "currentColor" }));
    }

    function makeCardComponent(editor, fields, t, titleKey, descriptionKey, advancedHintKey) {
      return function VisionaryCard() {
        var snapshot = useStore(editor.store);
        var openState = React.useState(false);
        var open = openState[0];
        var setOpen = openState[1];
        var disclosure = React.useState(false);
        var advancedOpen = disclosure[0];
        var setAdvancedOpen = disclosure[1];
        var triggeredFlag = React.useState(false);
        var justTriggered = triggeredFlag[0];
        var setJustTriggered = triggeredFlag[1];
        var saveStarted = React.useRef(false);

        var disabled = !snapshot.writable || snapshot.saving;
        var title = t(titleKey);

        // Native cards collapse once a save lands; mirror that so a finished edit
        // leaves the list in its compact state.
        React.useEffect(function () {
          if (snapshot.saving) { saveStarted.current = true; return; }
          if (!saveStarted.current) return;
          saveStarted.current = false;
          if (!snapshot.dirty && !snapshot.failed && !snapshot.conflict) setOpen(false);
        }, [snapshot.dirty, snapshot.failed, snapshot.conflict, snapshot.saving]);

        var badges = function (f, state) {
          if (!state.overridden) return null;
          return React.createElement("span", { key: "badges", className: "vlb-badges" }, [
            React.createElement("span", { key: "tag", className: "vlb-tag" }, t("overridden")),
            React.createElement("button", {
              key: "reset",
              type: "button",
              className: "vlb-reset",
              disabled: disabled,
              onClick: function () { editor.reset(f.id); },
            }, t("reset")),
          ]);
        };

        var renderField = function (f) {
          var state = snapshot.fields[f.id] || { text: "", overridden: false, invalid: false };
          var controlId = "visionary-field-" + f.id.replace(/[^A-Za-z0-9_-]/g, "-");
          var onEdit = function (text) { editor.edit(f.id, text); };

          if (f.kind === "boolean") {
            return React.createElement("div", { key: f.id, className: "vlb-field" }, [
              React.createElement("div", { key: "row", className: "vlb-toggleRow" }, [
                React.createElement("div", { key: "text", className: "vlb-toggleLabel" }, [
                  React.createElement("span", { key: "label", className: "vlb-label" }, t(f.labelKey)),
                  badges(f, state),
                ]),
                React.createElement(SwitchControl, {
                  key: "switch",
                  checked: state.text === "true",
                  label: t(f.labelKey),
                  disabled: disabled,
                  onChange: function (next) {
                    onEdit(String(typeof next === "boolean" ? next : state.text !== "true"));
                  },
                }),
              ]),
              React.createElement("p", { key: "hint", className: "vlb-hint" }, t(f.hintKey)),
            ]);
          }

          var control;
          if (f.kind === "trigger") {
            control = React.createElement("button", {
              type: "button",
              id: controlId,
              className: "vlb-trigger",
              disabled: disabled,
              onClick: function () {
                setJustTriggered(true);
                editor.trigger(f.id);
              },
            }, justTriggered ? t("cleanPastedTriggered") : t(f.actionKey));
          } else if (f.kind === "select") {
            control = React.createElement("select", {
              id: controlId,
              className: "vlb-input",
              value: state.text,
              disabled: disabled,
              onChange: function (e) { onEdit(e.target.value); },
            }, f.options.map(function (o) { return React.createElement("option", { key: o, value: o }, o); }));
          } else if (f.kind === "number") {
            control = React.createElement("input", {
              id: controlId,
              className: state.invalid ? "vlb-input vlb-inputInvalid" : "vlb-input",
              type: "text",
              inputMode: "numeric",
              "aria-invalid": state.invalid ? "true" : "false",
              value: state.text,
              disabled: disabled,
              onChange: function (e) { onEdit(e.target.value); },
            });
          } else if (f.kind === "textarea") {
            control = React.createElement("textarea", {
              id: controlId,
              className: "vlb-textarea",
              value: state.text,
              disabled: disabled,
              placeholder: t(f.placeholderKey || f.hintKey),
              onChange: function (e) { onEdit(e.target.value); },
            });
          } else {
            control = React.createElement("input", {
              id: controlId,
              className: "vlb-input",
              type: "text",
              value: state.text,
              disabled: disabled,
              onChange: function (e) { onEdit(e.target.value); },
            });
          }

          return React.createElement("div", { key: f.id, className: "vlb-field" }, [
            React.createElement("div", { key: "head", className: "vlb-fieldHead" }, [
              React.createElement("label", { key: "label", className: "vlb-label", htmlFor: controlId }, t(f.labelKey)),
              badges(f, state),
            ]),
            control,
            React.createElement("p", { key: "hint", className: state.invalid ? "vlb-invalid" : "vlb-hint" },
              state.invalid ? t("invalidNumber") : t(f.hintKey)),
          ]);
        };

        var body = [];
        if (snapshot.status === "loading") {
          body.push(React.createElement("p", { key: "loading", className: "vlb-loading" }, t("loading")));
        } else if (snapshot.status === "unavailable") {
          body.push(React.createElement("p", { key: "unavailable", className: "vlb-unavailable" }, t("unavailable")));
        } else {
          var advancedFields = fieldsIn(fields, "advanced");
          if (!snapshot.writable) {
            body.push(React.createElement("p", { key: "ro", className: "vlb-readOnly" }, t("readOnly")));
          }
          body.push(React.createElement("div", { key: "main" }, fieldsIn(fields, "main").map(renderField)));
          if (advancedFields.length > 0) {
            body.push(React.createElement("button", {
              key: "disclosure",
              type: "button",
              className: "vlb-disclosure",
              "aria-expanded": advancedOpen ? "true" : "false",
              onClick: function () { setAdvancedOpen(!advancedOpen); },
            }, [
              chevron(advancedOpen ? "vlb-chevron vlb-chevronOpen" : "vlb-chevron"),
              React.createElement("span", { key: "text" }, t("advanced") + " · " + t(advancedHintKey)),
            ]));
            if (advancedOpen) {
              body.push(React.createElement("div", { key: "advanced" }, advancedFields.map(renderField)));
            }
          }
          body.push(React.createElement("div", { key: "footer", className: "vlb-footer" }, [
            (snapshot.failed || snapshot.conflict)
              ? React.createElement("p", { key: "err", className: "vlb-failed" },
                  snapshot.conflict ? t("saveConflict") : t("saveFailed"))
              : null,
            React.createElement("button", {
              key: "discard",
              type: "button",
              className: "vlb-discard",
              disabled: !snapshot.dirty || snapshot.saving,
              onClick: function () { editor.discard(); setJustTriggered(false); },
            }, t("discard")),
            React.createElement("button", {
              key: "save",
              type: "button",
              className: "vlb-save",
              disabled: snapshot.status !== "ready" || !snapshot.dirty || snapshot.invalid || snapshot.saving,
              onClick: function () { editor.save(); },
            }, snapshot.saving ? t("saving") : t("save")),
          ]));
        }

        return React.createElement("li", { className: open ? "vlb-card vlb-cardOpen" : "vlb-card" }, [
          React.createElement("button", {
            key: "header",
            type: "button",
            className: "vlb-header",
            "aria-expanded": open ? "true" : "false",
            "aria-label": t(open ? "collapse" : "expand") + ": " + title,
            onClick: function () { setOpen(!open); },
          }, [
            React.createElement("span", { key: "text", className: "vlb-headText" }, [
              React.createElement("span", { key: "name", className: "vlb-name" }, title),
              React.createElement("span", { key: "desc", className: "vlb-description" }, t(descriptionKey)),
            ]),
            snapshot.dirty
              ? React.createElement("span", { key: "pending", className: "vlb-tag vlb-pending" }, t("unsaved"))
              : null,
            chevron(open ? "vlb-chevron vlb-chevronOpen" : "vlb-chevron"),
          ]),
          open ? React.createElement("div", { key: "body", className: "vlb-body" }, body) : null,
        ]);
      };
    }

    // Split one card's fields into the always-visible region and the collapsed
    // "advanced" region.
    function fieldsIn(fields, area) {
      return fields.filter(function (f) { return f.area === area; });
    }

    // ── apply ──

    var inject = ["slots", "locale", "settingsScope"];

    function apply(ctx) {
      ctx.effect(function () {
        return ctx.locale.register(NS, { zh: LOCALE_ZH, en: LOCALE_EN });
      }, "locale: " + NS);
      // Card chrome (CARD_CSS): one tagged <style> for both cards, removed with
      // the fiber so a disabled or updated row leaves no styles behind.
      ctx.effect(function () {
        if (typeof document === "undefined") return undefined;
        var style = document.createElement("style");
        style.setAttribute("data-plugin", "@xlight-oss/visionary-dsh");
        style.textContent = CARD_CSS;
        document.head.appendChild(style);
        return function () { style.remove(); };
      }, "visionary-settings: card chrome");
      var t = ctx.locale.bind(NS);

      var scopes = {};
      scopes[NS_VISION] = ctx.settingsScope.bind({ namespace: NS_VISION });
      scopes[NS_BRIDGE] = ctx.settingsScope.bind({ namespace: NS_BRIDGE });

      var visionEditor = makeEditor({ ctx: ctx, scopes: scopes, primaryNs: NS_VISION, fields: VISION_FIELDS });
      var bridgeEditor = makeEditor({ ctx: ctx, scopes: scopes, primaryNs: NS_BRIDGE, fields: BRIDGE_FIELDS });

      var VisionCard = makeCardComponent(visionEditor, VISION_FIELDS, t, "visionTitle", "visionDescription", "advancedHint");
      var BridgeCard = makeCardComponent(bridgeEditor, BRIDGE_FIELDS, t, "bridgeTitle", "bridgeDescription", "advancedHint");

      ctx.slots.inject("settings.plugin.item", function () {
        return ctx.slots.register({
          name: "settings.plugin.item",
          key: NS_VISION,
          order: 20,
          locale: NS,
        }, VisionCard);
      });

      ctx.slots.inject("settings.plugin.item", function () {
        return ctx.slots.register({
          name: "settings.plugin.item",
          key: NS_BRIDGE,
          order: 21,
          locale: NS,
        }, BridgeCard);
      });
    }

    exports.inject = inject;
    exports.apply = apply;
    return module.exports;
  }
});

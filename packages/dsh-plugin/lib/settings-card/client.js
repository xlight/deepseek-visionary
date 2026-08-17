window.__ModuleLoader__.load({
  id: "@xlight-oss/visionary-dsh/settings-card",
  factory: (require) => {
    var module = { exports: {} };
    var exports = module.exports;
    Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
    var React = require("react");
    var _dsh_client_runtime = require("@deepseek-ai/dsh-client-runtime/client");

    // ── namespaces served by the host /visionary/api route ──

    var NS_BRIDGE = "visionary-image-bridge";
    var NS_VISION = "visionary-vision";

    // ── field definitions: one merged page over both namespace schemas ──
    // `id` is the unique working key. Single-target fields carry `ns`/`key`;
    // shared fields carry `targets` and write the same logical setting to
    // several namespace keys at once (e.g. binaryPath is shared by the tools
    // and the bridge rows). `kind` drives the widget, `group` drives the
    // in-page group heading. Order = display order: core behavior first,
    // rarely-touched path config last.

    function field(ns, key, kind, opts) {
      return Object.assign(
        { ns: ns, key: key, id: ns + "." + key, kind: kind, targets: [{ ns: ns, key: key }] },
        opts,
      );
    }

    function sharedField(id, key, kind, opts) {
      return Object.assign({ id: id, key: key, kind: kind }, opts);
    }

    var FIELDS = [
      // 视觉工具（visionary-vision）：常用配置在前，二进制路径最后
      // （一个共享 binaryPath 同时作用于工具与桥接两个命名空间）
      field(NS_VISION, "modelType", "select", { group: "vision", options: ["vision", "ocr"], labelKey: "visionModelTypeLabel", hintKey: "visionModelTypeHint" }),
      field(NS_VISION, "visionTimeoutMs", "number", { group: "vision", labelKey: "visionTimeoutLabel", hintKey: "visionTimeoutHint" }),
      field(NS_VISION, "statusTimeoutMs", "number", { group: "vision", labelKey: "visionStatusTimeoutLabel", hintKey: "visionStatusTimeoutHint" }),
      field(NS_VISION, "loginTimeoutSeconds", "number", { group: "vision", labelKey: "visionLoginTimeoutLabel", hintKey: "visionLoginTimeoutHint" }),
      sharedField("shared.binaryPath", "binaryPath", "text", {
        group: "vision",
        labelKey: "sharedBinaryPathLabel",
        hintKey: "sharedBinaryPathHint",
        targets: [
          { ns: NS_VISION, key: "binaryPath" },
          { ns: NS_BRIDGE, key: "binaryPath" },
        ],
      }),
      // 图片桥接（visionary-image-bridge）：开关 → 行为 → 存储
      field(NS_BRIDGE, "enabled", "boolean", { group: "bridge", labelKey: "enabledLabel", hintKey: "enabledHint" }),
      field(NS_BRIDGE, "scope", "select", { group: "bridge", options: ["text-only", "also-vl"], labelKey: "scopeLabel", hintKey: "scopeHint" }),
      field(NS_BRIDGE, "mode", "select", { group: "bridge", options: ["agentic", "deterministic"], labelKey: "modeLabel", hintKey: "modeHint" }),
      field(NS_BRIDGE, "promptTemplate", "textarea", { group: "bridge", labelKey: "promptTemplateLabel", hintKey: "promptTemplateHint" }),
      field(NS_BRIDGE, "pastedDir", "text", { group: "bridge", labelKey: "pastedDirLabel", hintKey: "pastedDirHint" }),
      field(NS_BRIDGE, "retainHours", "number", { group: "bridge", labelKey: "retainHoursLabel", hintKey: "retainHoursHint" }),
    ];

    var FIELDS_BY_ID = {};
    FIELDS.forEach(function (f) { FIELDS_BY_ID[f.id] = f; });

    // ── locale keys ──

    var NS = "settings.plugins.visionary";

    var LOCALE_ZH = {
      nav: "Visionary",
      title: "Visionary",
      description: "DeepSeek Visionary：视觉识图 / OCR 工具（上传管道、超时）+ 文本模型图片桥接，修改即时生效",
      visionGroupLabel: "视觉工具",
      visionGroupDescription: "deepseek_vision / deepseek_ocr 等 5 个原生工具",
      bridgeGroupLabel: "图片桥接",
      bridgeGroupDescription: "纯文本模型粘贴图片自动放行 + 改写为文本引导",
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
      save: "保存",
      saving: "保存中…",
      discard: "放弃修改",
      unsaved: "未保存",
      saveFailed: "保存失败，已保留供修改。",
      saveConflict: "保存被拒绝：配置已在别处修改，请刷新后重试。",
      overridden: "已覆盖",
      readOnly: "本部署设置为只读。",
      loading: "加载中…",
      unavailable: "设置服务不可用。",
      invalidNumber: "请输入数字；留空表示使用默认值。",
    };

    var LOCALE_EN = {
      nav: "Visionary",
      title: "Visionary",
      description: "DeepSeek Visionary: vision/OCR tools (upload pipeline, timeouts) + text-model image bridge. Changes apply immediately",
      visionGroupLabel: "Vision Tools",
      visionGroupDescription: "deepseek_vision / deepseek_ocr and 3 more native tools",
      bridgeGroupLabel: "Image Bridge",
      bridgeGroupDescription: "Transparent image admission and rewrite for text-only models",
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
      save: "Save",
      saving: "Saving…",
      discard: "Discard",
      unsaved: "Unsaved",
      saveFailed: "Save failed; values left for you to correct.",
      saveConflict: "Save refused: the config changed elsewhere. Refresh and retry.",
      overridden: "Overridden",
      readOnly: "This deployment stores settings read-only.",
      loading: "Loading…",
      unavailable: "Settings service unavailable.",
      invalidNumber: "Enter a number, or leave blank to use the default.",
    };

    // ── private settings route client ──
    // The DSH settings RPC domain only serves allowlisted namespaces, so this
    // plugin's namespaces are read/written through the host's own fenced
    // /visionary/api routes (see ../settings-route.mjs). Each call carries the
    // target `ns`; the host defaults to the bridge namespace when absent. Any
    // failure (route forbidden, settings service absent, a value outside the
    // schema) surfaces as a rejected promise the scope turns into status.

    function callVisionaryApi(method, payload) {
      return fetch("/visionary/api/" + method, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {}),
      })
        .then(function (response) {
          return response.json().catch(function () { return null; });
        })
        .then(function (parsed) {
          if (parsed === null || parsed.ok !== true || parsed.value === undefined) {
            var code = (parsed && parsed.error && parsed.error.code) || "http";
            var message = (parsed && parsed.error && parsed.error.message) || "bad response";
            var err = new Error(message);
            err.code = code;
            throw err;
          }
          return parsed.value;
        });
    }

    // ── scope: one snapshot store per namespace ──

    function makeScope(ns) {
      var scope = _dsh_client_runtime.createSnapshotStore({
        status: "loading",
        value: undefined,
        base: undefined,
        user: undefined,
        revision: undefined,
        writable: false,
      });
      var generation = 0;
      var lastError = null;
      var tail = Promise.resolve();

      var applyView = function (view) {
        scope.set({
          status: "ready",
          value: view.value,
          base: view.base,
          user: view.user,
          revision: view.revision,
          writable: view.writable,
        });
      };

      var load = function () {
        var gen = ++generation;
        var op = callVisionaryApi("settings.get", { ns: ns }).then(function (view) {
          if (gen !== generation) return;
          applyView(view);
        }).catch(function (err) {
          if (gen !== generation) return;
          lastError = err;
          scope.set({
            status: "unavailable",
            value: undefined,
            base: undefined,
            user: undefined,
            revision: undefined,
            writable: false,
          });
        });
        tail = op.catch(function () {});
        return op;
      };

      var write = function (patch, expectedRevision) {
        var gen = ++generation;
        var op = callVisionaryApi("settings.update", {
          ns: ns,
          patch: patch,
          ...(expectedRevision !== undefined ? { expectedRevision: expectedRevision } : {}),
        }).then(function (view) {
          if (gen !== generation) return;
          applyView(view);
        }).catch(function (err) {
          if (gen !== generation) return;
          lastError = err;
          throw err;
        });
        tail = op.catch(function () {});
        return op;
      };

      var mutate = function (ops, expectedRevision) {
        var gen = ++generation;
        var op = callVisionaryApi("settings.mutate", {
          ns: ns,
          ops: ops,
          ...(expectedRevision !== undefined ? { expectedRevision: expectedRevision } : {}),
        }).then(function (view) {
          if (gen !== generation) return;
          applyView(view);
        }).catch(function (err) {
          if (gen !== generation) return;
          lastError = err;
          throw err;
        });
        tail = op.catch(function () {});
        return op;
      };

      load();

      return {
        scope: scope,
        getSnapshot: function () { return scope.getSnapshot(); },
        subscribe: function (fn) { return scope.subscribe(fn); },
        reload: load,
        write: write,
        mutate: mutate,
        lastError: function () { return lastError; },
      };
    }

    // ── inject factory for the settings.section slot ──
    // `scopes` maps each namespace to its own scope; the merged page combines
    // their snapshots. Saving splits the staged edits back to the owning
    // namespace (each write uses that namespace's own revision).

    function makeSectionInject(ctx, scopes, fields) {
      var t = ctx.locale.bind(NS);
      var staged = {};
      var saving = false;
      var failed = false;
      var conflict = false;
      var listeners = new Set();
      var cache = null;

      var emit = function () {
        listeners.forEach(function (l) { return l(); });
      };

      var snapshotOf = function (ns) { return scopes[ns].getSnapshot(); };

      // Shared fields write the same logical setting to several namespace keys
      // (e.g. binaryPath). Display value: first non-empty target; overridden:
      // any target's user layer holds the key.
      var fieldDisplayValue = function (f) {
        for (var i = 0; i < f.targets.length; i++) {
          var t = f.targets[i];
          var raw = (snapshotOf(t.ns).value || {})[t.key];
          if (raw !== undefined && raw !== null && raw !== "") return String(raw);
        }
        return "";
      };
      var fieldOverridden = function (f) {
        for (var i = 0; i < f.targets.length; i++) {
          var t = f.targets[i];
          var u = snapshotOf(t.ns).user;
          if (u !== undefined && Object.prototype.hasOwnProperty.call(u, t.key)) return true;
        }
        return false;
      };

      var rebuild = function () {
        var statuses = Object.keys(scopes).map(function (ns) { return snapshotOf(ns).status; });
        var ready = statuses.every(function (s) { return s === "ready"; });
        var unavailable = statuses.every(function (s) { return s === "unavailable"; });
        var writable = Object.keys(scopes).every(function (ns) { return snapshotOf(ns).writable; });
        if (!ready) {
          cache = {
            available: ready,
            status: unavailable ? "unavailable" : "loading",
            writable: writable,
            dirty: Object.keys(staged).length > 0,
            invalid: false,
            saving: saving,
            failed: failed,
            conflict: conflict,
            fields: {},
            value: {},
            user: undefined,
          };
          return;
        }
        var value = {};
        var user = {};
        Object.keys(scopes).forEach(function (ns) {
          var s = snapshotOf(ns);
          Object.assign(value, s.value || {});
          Object.assign(user, s.user || {});
        });
        var fieldsOut = {};
        var invalid = false;
        fields.forEach(function (f) {
          var st = Object.prototype.hasOwnProperty.call(staged, f.id) ? staged[f.id] : undefined;
          var text;
          var overridden;
          var fieldInvalid = false;
          if (st !== undefined) {
            text = st.text;
            overridden = st.text !== fieldDisplayValue(f);
            if (f.kind === "number") {
              var trimmed = st.text.trim();
              if (trimmed !== "") fieldInvalid = !Number.isFinite(Number(trimmed));
            }
          } else {
            text = fieldDisplayValue(f);
            overridden = fieldOverridden(f);
          }
          if (fieldInvalid) invalid = true;
          fieldsOut[f.id] = { text: text, overridden: overridden, invalid: fieldInvalid };
        });
        cache = {
          available: true,
          status: "ready",
          writable: writable,
          dirty: Object.keys(staged).length > 0,
          invalid: invalid,
          saving: saving,
          failed: failed,
          conflict: conflict,
          fields: fieldsOut,
          value: value,
          user: user,
        };
      };

      var publish = function () { rebuild(); emit(); };

      ctx.effect(function () {
        var cleanups = Object.keys(scopes).map(function (ns) {
          return scopes[ns].subscribe(function () { publish(); });
        });
        return function () { cleanups.forEach(function (cancel) { cancel(); }); };
      }, "visionary-settings-card: namespace scope subscriptions");

      rebuild();

      var store = {
        getSnapshot: function () { return cache; },
        subscribe: function (fn) {
          listeners.add(fn);
          return function () { listeners.delete(fn); };
        },
      };

      var stage = function (fieldId, text) {
        var f = FIELDS_BY_ID[fieldId];
        if (text === fieldDisplayValue(f)) delete staged[fieldId];
        else staged[fieldId] = { text: text };
        publish();
      };

      var resetField = function (fieldId) {
        var f = FIELDS_BY_ID[fieldId];
        if (fieldDisplayValue(f) === "") delete staged[fieldId];
        else staged[fieldId] = { text: "" };
        publish();
      };

      var coerce = function (fieldId, text) {
        var f = FIELDS_BY_ID[fieldId];
        if (!f) return text;
        if (f.kind === "boolean") return text === "true";
        if (f.kind === "number") return Number(text);
        return text;
      };

      var save = async function () {
        saving = true;
        failed = false;
        conflict = false;
        publish();
        var byNs = {};
        for (var fieldId in staged) {
          if (!Object.prototype.hasOwnProperty.call(staged, fieldId)) continue;
          var f = FIELDS_BY_ID[fieldId];
          var entry = staged[fieldId];
          f.targets.forEach(function (t) {
            var group = byNs[t.ns] || (byNs[t.ns] = { patch: {}, ops: [] });
            if (entry.text.trim() === "") {
              group.ops.push({ op: "unset", path: [t.key] });
            } else {
              group.patch[t.key] = coerce(fieldId, entry.text.trim());
            }
          });
        }
        try {
          for (var ns in byNs) {
            var g = byNs[ns];
            var s = snapshotOf(ns);
            if (g.ops.length > 0) await scopes[ns].mutate(g.ops, s.revision);
            if (Object.keys(g.patch).length > 0) {
              var after = s.revision;
              if (g.ops.length > 0) after = snapshotOf(ns).revision;
              await scopes[ns].write(g.patch, after);
            }
          }
          staged = {};
        } catch (err) {
          if (err && err.code === "settings-conflict") conflict = true;
          else failed = true;
        }
        saving = false;
        publish();
      };

      var discard = function () {
        staged = {};
        failed = false;
        conflict = false;
        publish();
      };

      return {
        hooks: { scope: store },
        edit: function (fieldId, text) { stage(fieldId, text); },
        resetField: resetField,
        save: save,
        discard: discard,
      };
    }

    // ── settings section component ──

    function VisionarySection(props) {
      var t = props.t;
      var snapshot = props.useScope(function (s) { return s; });

      var itemStyle = { display: "flex", flexDirection: "column", gap: 20, padding: "4px 2px" };
      var titleStyle = { margin: 0, color: "var(--dsw-alias-label-primary)", fontSize: 20, fontWeight: 700, lineHeight: 1.4 };
      var descStyle = { margin: 0, color: "var(--dsw-alias-label-tertiary)", fontSize: 14, lineHeight: 1.6 };
      var cardStyle = { border: "1px solid var(--dsw-alias-border-l2)", background: "var(--dsw-alias-bg-layer-3)", borderRadius: 12, overflow: "hidden" };
      var bodyStyle = { padding: "4px 16px 16px" };
      var fieldStyle = { display: "flex", flexDirection: "column", gap: 6, padding: "14px 0" };
      var labelRow = { display: "flex", alignItems: "center", gap: 8 };
      var labelStyle = { minWidth: 0, color: "var(--dsw-alias-label-primary)", flex: 1, fontSize: 13, fontWeight: 500, lineHeight: 1.5 };
      var inputStyle = { boxSizing: "border-box", border: "1px solid var(--dsw-alias-border-l2)", background: "var(--dsw-alias-bg-layer-3)", font: "inherit", color: "var(--dsw-alias-label-primary)", borderRadius: 8, padding: "4px 12px", fontSize: 13, lineHeight: 1.5, width: "100%" };
      var textareaStyle = { minHeight: 72, resize: "vertical", paddingTop: 8, paddingBottom: 8 };
      Object.assign(textareaStyle, inputStyle);
      var hintStyle = { color: "var(--dsw-alias-label-tertiary)", margin: 0, fontSize: 12, lineHeight: 1.5 };
      var errStyle = { color: "var(--dsw-alias-label-error)", margin: 0, fontSize: 12, lineHeight: 1.5 };
      var footerStyle = { borderTop: "1px solid var(--dsw-alias-border-l2)", justifyContent: "flex-end", alignItems: "center", gap: 8, padding: "12px 0 4px", display: "flex" };
      var btnBase = { appearance: "none", font: "inherit", cursor: "pointer", border: "1px solid transparent", borderRadius: 8, padding: "5px 14px", fontSize: 13, lineHeight: 1.5 };
      var discardBtn = { borderColor: "var(--dsw-alias-border-l2)", color: "var(--dsw-alias-label-secondary)", background: "none" };
      Object.assign(discardBtn, btnBase);
      var saveBtn = { color: "#fff", background: "var(--dsw-alias-brand-primary, #1677ff)" };
      Object.assign(saveBtn, btnBase);

      var fieldsEl;
      if (snapshot.status === "loading") {
        fieldsEl = React.createElement("p", { style: { color: "var(--dsw-alias-label-tertiary)", fontSize: 13, margin: "16px 0" } }, t("loading"));
      } else if (snapshot.status === "unavailable") {
        fieldsEl = React.createElement("p", { style: { color: "var(--dsw-alias-label-error)", fontSize: 13, margin: "16px 0" } }, t("unavailable"));
      } else {
        var groupTitleStyle = { margin: 0, color: "var(--dsw-alias-label-primary)", fontSize: 14, fontWeight: 600, lineHeight: 1.5 };
        var groupDescStyle = { margin: "2px 0 0", color: "var(--dsw-alias-label-tertiary)", fontSize: 12, lineHeight: 1.5 };
        var groupHeaderStyle = { display: "flex", flexDirection: "column", gap: 2, padding: "18px 0 2px", borderTop: "1px solid var(--dsw-alias-border-l2)" };
        var firstGroupStyle = { display: "flex", flexDirection: "column", gap: 2, padding: "2px 0 4px" };
        fieldsEl = [];
        var lastGroup = null;
        FIELDS.forEach(function (f) {
          if (f.group !== lastGroup) {
            lastGroup = f.group;
            var headerStyle = fieldsEl.length === 0 ? firstGroupStyle : groupHeaderStyle;
            fieldsEl.push(
              React.createElement("header", { key: "group-" + f.group, style: headerStyle }, [
                React.createElement("h3", { key: "title", style: groupTitleStyle }, t(f.group + "GroupLabel")),
                React.createElement("p", { key: "desc", style: groupDescStyle }, t(f.group + "GroupDescription")),
              ])
            );
          }
          var st = snapshot.fields[f.id] || { text: "", overridden: false, invalid: false };
          var disabled = !snapshot.writable || snapshot.saving;
          var edit = function (text) { props.edit(f.id, text); };
          var inputEl;
          if (f.kind === "boolean") {
            inputEl = React.createElement("input", { type: "checkbox", checked: st.text === "true", disabled: disabled, onChange: function (e) { edit(String(e.target.checked)); } });
          } else if (f.kind === "select") {
            inputEl = React.createElement("select", { value: st.text, disabled: disabled, onChange: function (e) { edit(e.target.value); }, style: inputStyle },
              f.options.map(function (o) { return React.createElement("option", { key: o, value: o }, o); })
            );
          } else if (f.kind === "number") {
            inputEl = React.createElement("input", { type: "text", inputMode: "numeric", value: st.text, disabled: disabled, placeholder: t("invalidNumber"), onChange: function (e) { edit(e.target.value); }, style: Object.assign({}, inputStyle, { borderColor: st.invalid ? "var(--dsw-alias-label-error)" : undefined }) });
          } else if (f.kind === "textarea") {
            inputEl = React.createElement("textarea", { value: st.text, disabled: disabled, placeholder: t("promptTemplateHint"), onChange: function (e) { edit(e.target.value); }, style: textareaStyle });
          } else {
            inputEl = React.createElement("input", { type: "text", value: st.text, disabled: disabled, onChange: function (e) { edit(e.target.value); }, style: inputStyle });
          }
          fieldsEl.push(React.createElement("div", { key: f.id, style: fieldStyle }, [
            React.createElement("div", { key: "head", style: labelRow }, [
              React.createElement("label", { key: "label", style: labelStyle }, t(f.labelKey)),
              st.overridden ? React.createElement("span", { key: "badge", style: { background: "var(--dsw-alias-bg-module-platform)", color: "var(--dsw-alias-label-secondary)", borderRadius: 999, padding: "1px 8px", fontSize: 11, fontWeight: 500, lineHeight: "17px", whiteSpace: "nowrap" } }, t("overridden")) : null,
            ]),
            React.createElement("div", { key: "ctrl", style: { width: "100%" } }, inputEl),
            React.createElement("p", { key: "hint", style: st.invalid ? errStyle : hintStyle }, st.invalid ? t("invalidNumber") : t(f.hintKey)),
          ]));
        });
      }

      var blocked = snapshot.status !== "ready" || !snapshot.dirty || snapshot.invalid || snapshot.saving;

      return React.createElement("div", { style: itemStyle }, [
        React.createElement("header", { key: "head", style: { display: "flex", flexDirection: "column", gap: 4 } }, [
          React.createElement("h2", { key: "title", style: titleStyle }, t("title")),
          React.createElement("p", { key: "desc", style: descStyle }, t("description")),
        ]),
        React.createElement("div", { key: "card", style: cardStyle }, [
          React.createElement("div", { key: "body", style: bodyStyle }, [
            !snapshot.writable && snapshot.status === "ready"
              ? React.createElement("p", { key: "ro", style: { color: "var(--dsw-alias-label-tertiary)", margin: "12px 0 0", fontSize: 12, lineHeight: 1.5 } }, t("readOnly"))
              : null,
            fieldsEl,
            (snapshot.failed || snapshot.conflict) && snapshot.status === "ready"
              ? React.createElement("p", { key: "err", style: { color: "var(--dsw-alias-label-error)", flex: 1, minWidth: 0, margin: 0, fontSize: 12, lineHeight: 1.5 } }, snapshot.conflict ? t("saveConflict") : t("saveFailed"))
              : null,
            React.createElement("div", { key: "footer", style: footerStyle }, [
              React.createElement("button", { key: "discard", type: "button", disabled: !snapshot.dirty || snapshot.saving, onClick: props.discard, style: discardBtn }, t("discard")),
              React.createElement("button", { key: "save", type: "button", disabled: blocked, onClick: props.save, style: saveBtn }, snapshot.saving ? t("saving") : t("save")),
            ]),
          ]),
        ]),
      ]);
    }

    // ── apply ──

    var inject = ["slots", "locale"];

    function apply(ctx) {
      ctx.effect(function () { return ctx.locale.register(NS, { zh: LOCALE_ZH, en: LOCALE_EN }); }, "locale: " + NS);

      var scopes = {};
      scopes[NS_VISION] = makeScope(NS_VISION);
      scopes[NS_BRIDGE] = makeScope(NS_BRIDGE);
      var t = ctx.locale.bind(NS);

      ctx.slots.inject("settings.section", function () {
        return ctx.slots.register({
          name: "settings.section",
          id: "visionary",
          order: 80,
          label: function () { return t("nav"); },
          locale: NS,
          inject: function () { return makeSectionInject(ctx, scopes, FIELDS); },
        }, VisionarySection);
      });
    }

    exports.inject = inject;
    exports.apply = apply;
    return module.exports;
  }
});
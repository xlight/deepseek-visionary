// Browser half of the visionary-dsh plugin (DSH 0.2.0 settings surface).
//
// DSH 0.2.0 exposes editable settings as *volatile fields of the profile entry
// config*: the host serves them through `ctx.settings.describe/update/mutate`
// (keyed by profile entry id, `SettingsConflictError` on stale writes), and the
// Plugins panel (`dsh-client-ui-plugin-manager`) renders `plugins.row.config`
// entries (keyed by `<bundle package>#<row id>`) as a Configure control on each
// bundle row, opening a page with a one-liner (`view: "summary"`) and a full
// form (`view: "page"`). This bundle ships two such row pages over the two host
// entries (`visionary-vision`, `visionary-image-bridge`) and stages edits through the
// primitives' `SettingsFormModel` + form controls (`SettingsValueField` /
// `Switch` / `SegmentedControl`), so the pages match the host's own
// shell/agent-loop pages exactly. There is no plugin-owned HTTP route and no
// snapshot-generation layer: the scope owns the wire, the revision fence, and
// the recovery read.
//
// Hand-written in the lazy-CJS bundle protocol (`window.__ModuleLoader__.load`)
// with the id equal to the package name (see the discovery note in this file's
// header). Module edges: `react` + `@deepseek-ai/dsh-client-ui-primitives` (both
// platform seed words, like every shipped settings page).
window.__ModuleLoader__.load({
  id: "@xlight-oss/visionary-dsh",
  factory: (require) => {
    var module = { exports: {} };
    var exports = module.exports;
    Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
    var React = require("react");
    var primitives = {};
    try {
      primitives = require("@deepseek-ai/dsh-client-ui-primitives") || {};
    } catch (error) {
      console.warn("visionary-dsh: UI primitives unavailable, settings pages will not render", error);
    }
    var SettingsForm = primitives.SettingsForm;
    var SettingsFormModel = primitives.SettingsFormModel;
    var SettingsValueField = primitives.SettingsValueField;
    var settingsNumberField = primitives.settingsNumberField;
    var settingsTextField = primitives.settingsTextField;
    var SwitchControl = primitives.Switch;
    var SegmentedControl = primitives.SegmentedControl;

    var NS = "settings.plugins.visionary";
    var NS_VISION = "visionary-vision";
    var NS_BRIDGE = "visionary-image-bridge";

    var LOCALE_ZH = {
      visionTitle: "Visionary · 视觉工具",
      visionDescription: "deepseek_vision / deepseek_ocr 等 6 个原生工具：上传管道、超时与二进制路径，修改即时生效",
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
      visionAccountsTimeoutLabel: "账号探测超时（毫秒）",
      visionAccountsTimeoutHint: "账号池批量探测预算；插件会在 CLI 预算之上留出余量",
      sharedBinaryPathLabel: "二进制路径",
      sharedBinaryPathHint: "visionary-cli 路径，工具与桥接（deterministic 模式）共用；空 = DEEPSEEK_VISIONARY_BIN → PATH",
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
      routesLabel: "桥接路由",
      routesHint: "JSON 数组，[{ \"provider\": \"pi-ai\", \"model\": \"*\" }]；空数组 = 全部路由",
      promptTemplateLabel: "引导模板",
      promptTemplateHint: "必须包含 {path} 占位符",
      cleanPastedLabel: "清理已落盘副本",
      cleanPastedHint: "一次性操作：删除落盘目录中的图片副本（附件库不受影响），清理数量写入 DSH 日志",
      cleanPastedAction: "立即清理",
      cleanPastedTriggered: "已触发清理",
      save: "保存",
      saving: "保存中…",
      discard: "放弃修改",
      saveFailed: "保存失败，已保留供修改。",
      unavailable: "该插件当前未加载，暂时无法配置。",
      readOnly: "本部署的设置为只读。",
      overridden: "已覆盖",
      reset: "恢复默认",
      invalidNumber: "请输入数字；留空表示使用默认值。",
      invalidTemplate: "引导模板必须包含 {path} 占位符。",
      invalidRoutes: "路由需为 JSON 数组，如 [{\"provider\":\"pi-ai\",\"model\":\"*\"}]。",
      // ── 仪表盘段（plugins.bundle.config）──────────────────────────────────
      dashboardSummary: "二进制、登录与桥接运行态总览，含诊断动作",
      dashboardBinaryTitle: "二进制状态",
      dashboardBinaryReady: "就绪",
      dashboardBinaryMissing: "缺失",
      dashboardBinaryIncompatible: "版本不兼容",
      dashboardLoginTitle: "登录 / 会话",
      dashboardLoginOk: "已登录",
      dashboardLoginOff: "未登录",
      dashboardLoginError: "查询失败",
      dashboardBridgeTitle: "桥接运行态",
      dashboardBridgeOn: "启用",
      dashboardBridgeOff: "禁用",
      dashboardPastedCount: "落盘副本",
      dashboardActionsTitle: "诊断",
      dashboardReprobe: "重探测二进制",
      dashboardRelogin: "重新登录",
      dashboardClean: "清理落盘",
      dashboardSelfCheck: "自检",
      dashboardLoading: "加载中…",
      dashboardError: "出错",
      // ── 账号池区块 ────────────────────────────────────────────────────────
      accountsTitle: "账号池",
      accountsSummary: "共 {total} 个账号：正常 {healthy}、静音 {muted}、失效 {expired}",
      accountsCurrent: "当前使用",
      accountsHealthy: "正常",
      accountsMuted: "静音",
      accountsExpired: "失效",
      accountsPinned: "已固定",
      accountsLoading: "加载中…",
      accountsError: "出错",
      accountsEmpty: "未启用账号池。运行 visionary-cli accounts add 添加第一个账号。",
      accountsEnvIgnored: "已忽略环境变量凭据，当前按账号池轮询。",
      accountsRefresh: "刷新健康",
      accountsAdd: "添加账号",
      accountsAddStarted: "已启动添加账号流程，请在浏览器里完成登录；完成后刷新列表。",
      accountsRemove: "移除",
      accountsPin: "固定",
      accountsUnpin: "取消固定",
    };

    var LOCALE_EN = {
      visionTitle: "Visionary · Vision Tools",
      visionDescription: "deepseek_vision / deepseek_ocr and 4 more native tools: upload pipeline, timeouts and binary path. Changes apply immediately",
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
      visionAccountsTimeoutLabel: "Accounts timeout (ms)",
      visionAccountsTimeoutHint: "Account-pool health refresh budget; the plugin adds a safety margin over the CLI budget",
      sharedBinaryPathLabel: "Binary path",
      sharedBinaryPathHint: "visionary-cli binary, shared by the tools and the bridge (deterministic mode); empty = DEEPSEEK_VISIONARY_BIN → PATH",
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
      routesLabel: "Bridge routes",
      routesHint: "JSON array, e.g. [{\"provider\":\"pi-ai\",\"model\":\"*\"}]; empty = all routes",
      promptTemplateLabel: "Prompt template",
      promptTemplateHint: "Must contain the {path} placeholder",
      cleanPastedLabel: "Clean pasted copies",
      cleanPastedHint: "One-shot action: delete the pasted image copies (the attachment store is untouched); the removed count lands in the DSH log",
      cleanPastedAction: "Clean now",
      cleanPastedTriggered: "Cleanup triggered",
      save: "Save",
      saving: "Saving…",
      discard: "Discard",
      saveFailed: "Save failed; values left for you to correct.",
      unavailable: "This plugin is not loaded, so it cannot be configured right now.",
      readOnly: "This deployment stores settings read-only.",
      overridden: "Overridden",
      reset: "Reset to default",
      invalidNumber: "Enter a number, or leave blank to use the default.",
      invalidTemplate: "The prompt template must contain the {path} placeholder.",
      invalidRoutes: "Routes must be a JSON array, e.g. [{\"provider\":\"pi-ai\",\"model\":\"*\"}].",
      // ── dashboard section (plugins.bundle.config) ──────────────────────────
      dashboardSummary: "Binary, login and bridge runtime status with diagnostic actions",
      dashboardBinaryTitle: "Binary status",
      dashboardBinaryReady: "Ready",
      dashboardBinaryMissing: "Missing",
      dashboardBinaryIncompatible: "Version incompatible",
      dashboardLoginTitle: "Login / session",
      dashboardLoginOk: "Logged in",
      dashboardLoginOff: "Not logged in",
      dashboardLoginError: "Query failed",
      dashboardBridgeTitle: "Bridge runtime",
      dashboardBridgeOn: "Enabled",
      dashboardBridgeOff: "Disabled",
      dashboardPastedCount: "Pasted copies",
      dashboardActionsTitle: "Diagnostics",
      dashboardReprobe: "Re-probe binary",
      dashboardRelogin: "Re-login",
      dashboardClean: "Clean pasted",
      dashboardSelfCheck: "Self-check",
      dashboardLoading: "Loading…",
      dashboardError: "Error",
      // ── account pool section ─────────────────────────────────────────────
      accountsTitle: "Account pool",
      accountsSummary: "{total} account(s): healthy {healthy}, muted {muted}, expired {expired}",
      accountsCurrent: "Current account",
      accountsHealthy: "Healthy",
      accountsMuted: "Muted",
      accountsExpired: "Expired",
      accountsPinned: "Pinned",
      accountsLoading: "Loading…",
      accountsError: "Error",
      accountsEmpty: "Account pool not enabled. Run visionary-cli accounts add to add the first account.",
      accountsEnvIgnored: "Environment credentials ignored — rotation uses the account pool only.",
      accountsRefresh: "Refresh health",
      accountsAdd: "Add account",
      accountsAddStarted: "Add-account flow started. Finish logging in in the browser, then refresh the list.",
      accountsRemove: "Remove",
      accountsPin: "Pin",
      accountsUnpin: "Unpin",
    };

    /** Copy the frame labels the shared SettingsForm wants from this page. */
    function formLabels(t) {
      return {
        unavailable: t("unavailable"),
        readOnly: t("readOnly"),
        saveFailed: t("saveFailed"),
        save: t("save"),
        saving: t("saving"),
      };
    }

    // ── field specs ───────────────────────────────────────────────────────────

    function choiceSpec(field, allowed) {
      return {
        field,
        format: (value) => (allowed.includes(value) ? value : allowed[0]),
        parse: (text) => (allowed.includes(text) ? { kind: "set", value: text } : void 0),
      };
    }

    function booleanSpec(field) {
      return {
        field,
        format: (value) => String(value === true),
        parse: (text) => ({ kind: "set", value: text === "true" }),
      };
    }

    // Fail-loud: a template without {path} parses to undefined → the field is
    // invalid and the form refuses to save (replaces the 0.1.5 validate hook).
    function pathTemplateSpec(field) {
      return {
        field,
        format: (value) => (typeof value === "string" ? value : ""),
        parse: (text) => (text.includes("{path}") ? { kind: "set", value: text } : void 0),
      };
    }

    // routes 是数组字段，表单里按 JSON 文本编辑（高级区）。
    function jsonListSpec(field) {
      return {
        field,
        format: (value) => (Array.isArray(value) ? JSON.stringify(value) : ""),
        parse: (text) => {
          const trimmed = text.trim();
          if (trimmed === "") return { kind: "clear" };
          try {
            const parsed = JSON.parse(trimmed);
            return Array.isArray(parsed) ? { kind: "set", value: parsed } : void 0;
          } catch {
            return void 0;
          }
        },
      };
    }

    // ── row chrome (tokens only, mirrors the SettingsValueField layout) ───────

    var styles = {
      fieldRow: { display: "flex", flexDirection: "column", gap: 6, padding: "12px 0" },
      fieldDivider: { borderTop: ".5px solid var(--dsw-alias-border-l2)" },
      head: { display: "flex", alignItems: "center", gap: 8 },
      label: { minWidth: 0, flex: 1, color: "var(--dsw-alias-label-primary)", fontSize: 13, fontWeight: 500, lineHeight: 1.5 },
      badges: { display: "inline-flex", alignItems: "center", gap: 8 },
      tag: { borderRadius: 999, padding: "1px 8px", fontSize: 11, lineHeight: "17px", fontWeight: 500, whiteSpace: "nowrap", background: "var(--dsw-alias-bg-module-platform)", color: "var(--dsw-alias-label-secondary)" },
      // 账号池状态徽标：沿用 dashboard 手写 styles 族（宿主没有 warning / neutral 徽标 token）。
      tagWarning: { borderRadius: 999, padding: "1px 8px", fontSize: 11, lineHeight: "17px", fontWeight: 500, whiteSpace: "nowrap", background: "var(--dsw-alias-bg-warning, var(--dsw-alias-bg-module-platform))", color: "var(--dsw-alias-label-warning, var(--dsw-alias-label-secondary))" },
      tagNeutral: { borderRadius: 999, padding: "1px 8px", fontSize: 11, lineHeight: "17px", fontWeight: 500, whiteSpace: "nowrap", background: "transparent", border: "1px solid var(--dsw-alias-border-l2)", color: "var(--dsw-alias-label-tertiary)" },
      reset: { font: "inherit", color: "var(--dsw-alias-label-secondary)", cursor: "pointer", background: "0 0", border: "none", padding: 0, fontSize: 12, lineHeight: 1.5 },
      hint: { margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--dsw-alias-label-tertiary)" },
      invalid: { margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--dsw-alias-label-error)" },
      boolRow: { display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 16 },
      boolText: { flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 6 },
      disclosure: { appearance: "none", width: "100%", font: "inherit", textAlign: "left", cursor: "pointer", background: "0 0", border: 0, borderTop: ".5px solid var(--dsw-alias-border-l2)", color: "var(--dsw-alias-label-secondary)", display: "flex", alignItems: "center", gap: 6, padding: "12px 0", fontSize: 13, lineHeight: 1.5 },
      trigger: { appearance: "none", font: "inherit", cursor: "pointer", border: "1px solid var(--dsw-alias-border-l2)", color: "var(--dsw-alias-label-secondary)", background: "0 0", borderRadius: 8, padding: "5px 14px", fontSize: 13, lineHeight: 1.5, alignSelf: "flex-start" },
    };

    // Same glyph as the host's IconChevronDownOutline14, inlined.
    var CHEVRON_PATH = "M11.8486 5.5L11.4238 5.92383L8.69727 8.65137C8.44157 8.90706 8.21562 9.13382 8.01172 9.29785C7.79912 9.46883 7.55595 9.61756 7.25 9.66602C7.08435 9.69222 6.91565 9.69222 6.75 9.66602C6.44405 9.61756 6.20088 9.46883 5.98828 9.29785C5.78438 9.13382 5.55843 8.90706 5.30273 8.65137L2.57617 5.92383L2.15137 5.5L3 4.65137L3.42383 5.07617L6.15137 7.80273C6.42595 8.07732 6.59876 8.24849 6.74023 8.3623C6.87291 8.46904 6.92272 8.47813 6.9375 8.48047C6.97895 8.48703 7.02105 8.48703 7.0625 8.48047C7.07728 8.47813 7.12709 8.46904 7.25977 8.3623C7.40124 8.24849 7.57405 8.07732 7.84863 7.80273L10.5762 5.07617L11 4.65137L11.8486 5.5Z";

    function chevron(open) {
      return React.createElement("svg", {
        width: 14,
        height: 14,
        viewBox: "0 0 14 14",
        fill: "none",
        "aria-hidden": "true",
        style: { flex: "none", color: "var(--dsw-alias-label-tertiary)", transition: "transform .16s", transform: open ? "rotate(180deg)" : "none" },
      }, React.createElement("path", { d: CHEVRON_PATH, fill: "currentColor" }));
    }

    function badgesRow(fieldState, disabled, t, onReset) {
      if (!fieldState.overridden) return null;
      return React.createElement("span", { className: "vlz-badges", style: styles.badges }, [
        React.createElement("span", { key: "tag", style: styles.tag }, t("overridden")),
        React.createElement("button", {
          key: "reset",
          type: "button",
          style: styles.reset,
          disabled: disabled,
          onClick: onReset,
        }, t("reset")),
      ]);
    }

    /** One labeled row with an overridden badge + reset next to the label. */
    function fieldHead(labelText, fieldState, disabled, t, onReset) {
      return React.createElement("div", { style: styles.head }, [
        React.createElement("span", { key: "label", style: styles.label }, labelText),
        badgesRow(fieldState, disabled, t, onReset),
      ]);
    }

    function hint(text, invalid) {
      return React.createElement("p", { style: invalid ? styles.invalid : styles.hint }, text);
    }

    // ── cards ─────────────────────────────────────────────────────────────────

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

    // Vision card: five fields (binaryPath is dual-written to the bridge entry).
    function makeVisionCard(ctx, t) {
      var visionScope = ctx.configForms.get(NS_VISION);
      var bridgeScope = ctx.configForms.get(NS_BRIDGE);
      var form = new SettingsFormModel(visionScope, [
        choiceSpec("modelType", ["vision", "ocr"]),
        settingsNumberField("visionTimeoutMs"),
        settingsTextField("binaryPath"),
        settingsNumberField("statusTimeoutMs"),
        settingsNumberField("loginTimeoutSeconds"),
      ]);
      var bridgeBinary = new SettingsFormModel(bridgeScope, [settingsTextField("binaryPath")]);
      var store = form.bind(function () {
        var shell = form.shell();
        var binary = bridgeBinary.shell();
        return {
          available: shell.available,
          writable: shell.writable && binary.writable,
          dirty: shell.dirty || binary.dirty,
          invalid: shell.invalid || binary.invalid,
          saving: shell.saving || binary.saving,
          failed: shell.failed || binary.failed,
          modelType: form.field("modelType"),
          visionTimeoutMs: form.field("visionTimeoutMs"),
          binaryPath: form.field("binaryPath"),
          statusTimeoutMs: form.field("statusTimeoutMs"),
          loginTimeoutSeconds: form.field("loginTimeoutSeconds"),
        };
      });
      function edit(field, text) {
        form.actions().edit(field, text);
        if (field === "binaryPath") bridgeBinary.actions().edit(field, text);
      }
      function resetField(field) {
        form.actions().resetField(field);
        if (field === "binaryPath") bridgeBinary.actions().resetField(field);
      }
      function discard() {
        form.actions().discard();
        bridgeBinary.actions().discard();
      }
      async function save() {
        var landed = await form.actions().save();
        if (!landed) return false;
        return bridgeBinary.actions().save();
      }
      return {
        dispose() { form.dispose(); bridgeBinary.dispose(); },
        inject() {
          return { hooks: { visionCard: store }, visionEdit: edit, visionResetField: resetField, visionSave: save, visionDiscard: discard };
        },
        view(path, props) { return props.view === "summary" ? t("visionDescription") : path; },
        raw: { form, store },
      };
    }

    // Bridge card: boolean/choice fields via Switch/SegmentedControl, numeric/text
    // via SettingsValueField, plus the one-shot cleanPasted action button.
    function makeBridgeCard(ctx, t) {
      var scope = ctx.configForms.get(NS_BRIDGE);
      var form = new SettingsFormModel(scope, [
        booleanSpec("enabled"),
        choiceSpec("scope", ["text-only", "also-vl"]),
        choiceSpec("mode", ["agentic", "deterministic"]),
        settingsTextField("binaryPath"),
        settingsTextField("pastedDir"),
        pathTemplateSpec("promptTemplate"),
        settingsNumberField("retainHours"),
        jsonListSpec("routes"),
      ]);
      var store = form.bind(function () {
        var shell = form.shell();
        return Object.assign({}, shell, {
          enabled: form.field("enabled"),
          scope: form.field("scope"),
          mode: form.field("mode"),
          binaryPath: form.field("binaryPath"),
          pastedDir: form.field("pastedDir"),
          promptTemplate: form.field("promptTemplate"),
          retainHours: form.field("retainHours"),
          routes: form.field("routes"),
        });
      });
      function triggerClean() {
        var snapshot = scope.getSnapshot();
        scope.mutate([{ op: "set", path: ["cleanPasted"], value: true }], snapshot.revision);
      }
      return {
        dispose() { form.dispose(); },
        inject() {
          return {
            hooks: { bridgeCard: store },
            bridgeEdit: form.actions().edit,
            bridgeResetField: form.actions().resetField,
            bridgeSave: form.actions().save,
            bridgeDiscard: form.actions().discard,
            bridgeTriggerClean: triggerClean,
          };
        },
        view(path, props) { return props.view === "summary" ? t("bridgeDescription") : path; },
        raw: { form, store },
      };
    }

    // ── page components ───────────────────────────────────────────────────────

    /** Build one numeric/text SettingsValueField row with stable per-row bindings. */
    function advanceRow(f, idPrefix, t, editFn, resetFn, disabled) {
      return React.createElement("div", { key: f.key, style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement(SettingsValueField, {
          id: idPrefix + f.key,
          label: f.label,
          hint: f.hint,
          overriddenLabel: t("overridden"),
          resetLabel: t("reset"),
          invalidLabel: f.invalidLabel,
          numeric: Boolean(f.numeric),
          disabled: disabled,
          ...f.field,
          onEdit: function (text) { editFn(f.edit, text); },
          onReset: function () { resetFn(f.edit); },
        }),
      ]);
    }
    function VisionaryCard(props) {
      var t = props.t;
      var snapshot = props.useVisionCard(function (s) { return s; });
      var advanced = React.useState(false);
      var advancedOpen = advanced[0];
      var setAdvancedOpen = advanced[1];
      if (props.view === "summary") return t("visionDescription");
      var disabled = !snapshot.writable;
      var children = [];

      children.push(React.createElement("div", { key: "modelType", style: styles.fieldRow }, [
        fieldHead(t("visionModelTypeLabel"), snapshot.modelType, disabled, t, function () { props.visionResetField("modelType"); }),
        React.createElement(SegmentedControl, {
          id: "visionary-modelType",
          label: t("visionModelTypeLabel"),
          value: snapshot.modelType.text,
          disabled: disabled,
          options: [{ value: "vision", label: "vision" }, { value: "ocr", label: "ocr" }],
          onChange: function (next) { props.visionEdit("modelType", next); },
        }),
        hint(t("visionModelTypeHint")),
      ]));

      children.push(React.createElement("div", { key: "binaryPath", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement(SettingsValueField, {
          id: "visionary-binaryPath",
          label: t("sharedBinaryPathLabel"),
          hint: t("sharedBinaryPathHint"),
          overriddenLabel: t("overridden"),
          resetLabel: t("reset"),
          invalidLabel: t("invalidNumber"),
          disabled: disabled,
          ...snapshot.binaryPath,
          onEdit: function (text) { props.visionEdit("binaryPath", text); },
          onReset: function () { props.visionResetField("binaryPath"); },
        }),
      ]));

      children.push(React.createElement("div", { key: "visionTimeoutMs", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement(SettingsValueField, {
          id: "visionary-visionTimeoutMs",
          label: t("visionTimeoutLabel"),
          hint: t("visionTimeoutHint"),
          overriddenLabel: t("overridden"),
          resetLabel: t("reset"),
          invalidLabel: t("invalidNumber"),
          numeric: true,
          disabled: disabled,
          ...snapshot.visionTimeoutMs,
          onEdit: function (text) { props.visionEdit("visionTimeoutMs", text); },
          onReset: function () { props.visionResetField("visionTimeoutMs"); },
        }),
      ]));

      children.push(React.createElement("button", {
        key: "disclosure",
        type: "button",
        style: styles.disclosure,
        "aria-expanded": advancedOpen ? "true" : "false",
        onClick: function () { setAdvancedOpen(!advancedOpen); },
      }, [
        chevron(advancedOpen),
        React.createElement("span", { key: "text" }, t("advanced") + " · " + t("advancedHint")),
      ]));

      if (advancedOpen) {
        var advancedVision = [
          { key: "statusTimeoutMs", label: t("visionStatusTimeoutLabel"), hint: t("visionStatusTimeoutHint"), field: snapshot.statusTimeoutMs, edit: "statusTimeoutMs", numeric: true, invalidLabel: t("invalidNumber") },
          { key: "loginTimeoutSeconds", label: t("visionLoginTimeoutLabel"), hint: t("visionLoginTimeoutHint"), field: snapshot.loginTimeoutSeconds, edit: "loginTimeoutSeconds", numeric: true, invalidLabel: t("invalidNumber") },
          { key: "accountsTimeoutMs", label: t("visionAccountsTimeoutLabel"), hint: t("visionAccountsTimeoutHint"), field: snapshot.accountsTimeoutMs, edit: "accountsTimeoutMs", numeric: true, invalidLabel: t("invalidNumber") },
        ];
        for (var i = 0; i < advancedVision.length; i++) {
          children.push(advanceRow(advancedVision[i], "visionary-", t, props.visionEdit, props.visionResetField, disabled));
        }
      }

      return React.createElement(SettingsForm, {
        labels: formLabels(t),
        state: snapshot,
        onSave: props.visionSave,
        onDiscard: props.visionDiscard,
      }, children);
    }

    function BridgeCard(props) {
      var t = props.t;
      var snapshot = props.useBridgeCard(function (s) { return s; });
      var advanced = React.useState(false);
      var advancedOpen = advanced[0];
      var setAdvancedOpen = advanced[1];
      var triggered = React.useState(false);
      var justTriggered = triggered[0];
      var setJustTriggered = triggered[1];
      if (props.view === "summary") return t("bridgeDescription");
      var disabled = !snapshot.writable;
      var children = [];

      children.push(React.createElement("div", { key: "enabled", style: styles.fieldRow }, [
        React.createElement("div", { style: styles.boolRow }, [
          React.createElement("div", { style: styles.boolText }, [
            fieldHead(t("enabledLabel"), snapshot.enabled, disabled, t, function () { props.bridgeResetField("enabled"); }),
          ]),
          React.createElement(SwitchControl, {
            checked: snapshot.enabled.text === "true",
            label: t("enabledLabel"),
            disabled: disabled,
            onChange: function (next) {
              props.bridgeEdit("enabled", String(typeof next === "boolean" ? next : snapshot.enabled.text !== "true"));
            },
          }),
        ]),
        hint(t("enabledHint")),
      ]));

      children.push(React.createElement("div", { key: "scope", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        fieldHead(t("scopeLabel"), snapshot.scope, disabled, t, function () { props.bridgeResetField("scope"); }),
        React.createElement(SegmentedControl, {
          id: "visionary-scope",
          label: t("scopeLabel"),
          value: snapshot.scope.text,
          disabled: disabled,
          options: [{ value: "text-only", label: "text-only" }, { value: "also-vl", label: "also-vl" }],
          onChange: function (next) { props.bridgeEdit("scope", next); },
        }),
        hint(t("scopeHint")),
      ]));

      children.push(React.createElement("div", { key: "mode", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        fieldHead(t("modeLabel"), snapshot.mode, disabled, t, function () { props.bridgeResetField("mode"); }),
        React.createElement(SegmentedControl, {
          id: "visionary-mode",
          label: t("modeLabel"),
          value: snapshot.mode.text,
          disabled: disabled,
          options: [{ value: "agentic", label: "agentic" }, { value: "deterministic", label: "deterministic" }],
          onChange: function (next) { props.bridgeEdit("mode", next); },
        }),
        hint(t("modeHint")),
      ]));

      children.push(React.createElement("div", { key: "binaryPath", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement(SettingsValueField, {
          id: "visionary-bridge-binaryPath",
          label: t("sharedBinaryPathLabel"),
          hint: t("sharedBinaryPathHint"),
          overriddenLabel: t("overridden"),
          resetLabel: t("reset"),
          invalidLabel: t("invalidNumber"),
          disabled: disabled,
          ...snapshot.binaryPath,
          onEdit: function (text) { props.bridgeEdit("binaryPath", text); },
          onReset: function () { props.bridgeResetField("binaryPath"); },
        }),
      ]));

      children.push(React.createElement("button", {
        key: "disclosure",
        type: "button",
        style: styles.disclosure,
        "aria-expanded": advancedOpen ? "true" : "false",
        onClick: function () { setAdvancedOpen(!advancedOpen); },
      }, [
        chevron(advancedOpen),
        React.createElement("span", { key: "text" }, t("advanced") + " · " + t("advancedHint")),
      ]));

      if (advancedOpen) {
        var rows = [
          { key: "pastedDir", label: t("pastedDirLabel"), hint: t("pastedDirHint"), field: snapshot.pastedDir, edit: "pastedDir", numeric: false, invalidLabel: t("invalidNumber") },
          { key: "promptTemplate", label: t("promptTemplateLabel"), hint: t("promptTemplateHint"), field: snapshot.promptTemplate, edit: "promptTemplate", numeric: false, invalidLabel: t("invalidTemplate") },
          { key: "retainHours", label: t("retainHoursLabel"), hint: t("retainHoursHint"), field: snapshot.retainHours, edit: "retainHours", numeric: true, invalidLabel: t("invalidNumber") },
          { key: "routes", label: t("routesLabel"), hint: t("routesHint"), field: snapshot.routes, edit: "routes", numeric: false, invalidLabel: t("invalidRoutes") },
        ];
        for (var i = 0; i < rows.length; i++) {
          children.push(advanceRow(rows[i], "visionary-bridge-", t, props.bridgeEdit, props.bridgeResetField, disabled));
        }
        children.push(React.createElement("div", { key: "cleanPasted", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
          fieldHead(t("cleanPastedLabel"), { overridden: false }, disabled, t, function () {}),
          React.createElement("button", {
            type: "button",
            style: styles.trigger,
            disabled: disabled,
            onClick: function () { setJustTriggered(true); props.bridgeTriggerClean(); },
          }, justTriggered ? t("cleanPastedTriggered") : t("cleanPastedAction")),
          hint(t("cleanPastedHint")),
        ]));
      }

      return React.createElement(SettingsForm, {
        labels: formLabels(t),
        state: snapshot,
        onSave: props.bridgeSave,
        onDiscard: props.bridgeDiscard,
      }, children);
    }

    // ── dashboard section (plugins.bundle.config) ─────────────────────────────
    //
    // Renders above the per-entry rows in the bundle detail page. Pulls
    // aggregated status from the host VisionaryDashboard Service via
    // ctx.remote.visionaryDashboard and exposes four diagnostic actions.

    function VisionaryDashboardSummary(props) {
      var t = props.t;
      var remote = props.remote;
      var state = React.useState({ status: null, error: null, loading: true, action: null, actionError: null });
      var snap = state[0];
      var setSnap = state[1];

      function refresh() {
        setSnap(function (prev) { return Object.assign({}, prev, { loading: true, error: null }); });
        // dashboard row 未启用时 remote 可能是 undefined——判空后静默返回，绝不抛错。
        if (!remote) {
          setSnap(function (prev) { return Object.assign({}, prev, { loading: false }); });
          return;
        }
        remote.getStatus().then(
          function (status) { setSnap({ status: status, error: null, loading: false, action: null, actionError: null }); },
          function (err) { setSnap({ status: null, error: String(err?.message ?? err), loading: false, action: null, actionError: null }); }
        );
      }

      React.useEffect(function () { refresh(); }, []);

      function runAction(name, fn) {
        setSnap(function (prev) { return Object.assign({}, prev, { action: name, actionError: null }); });
        Promise.resolve()
          .then(function () { return fn(); })
          .then(
            function () { refresh(); },
            function (err) { setSnap(function (prev) { return Object.assign({}, prev, { action: null, actionError: String(err?.message ?? err) }); }); }
          );
      }

      var children = [];

      if (snap.loading) {
        children.push(React.createElement("p", { key: "loading", style: styles.hint }, t("dashboardLoading")));
        return React.createElement("section", { style: styles.fieldRow }, children);
      }

      if (snap.error) {
        children.push(React.createElement("p", { key: "error", style: styles.invalid }, t("dashboardError") + ": " + snap.error));
        children.push(React.createElement("button", { key: "retry", type: "button", style: styles.trigger, onClick: refresh }, t("dashboardSelfCheck")));
        return React.createElement("section", { style: styles.fieldRow }, children);
      }

      var s = snap.status || {};

      // Binary status row
      var binary = s.binary || {};
      var binaryBadge;
      if (binary.ready && binary.compatible) {
        binaryBadge = React.createElement("span", { key: "b", style: styles.tag }, t("dashboardBinaryReady"));
      } else if (binary.ready && !binary.compatible) {
        binaryBadge = React.createElement("span", { key: "b", style: styles.tag }, t("dashboardBinaryIncompatible"));
      } else {
        binaryBadge = React.createElement("span", { key: "b", style: styles.tag }, t("dashboardBinaryMissing"));
      }
      var binaryDetail = binary.ready ? (binary.version || "") + (binary.compatible ? "" : " (expected " + (binary.expectedMinor || "?") + ")") : (binary.hint || "");
      children.push(React.createElement("div", { key: "binary", style: styles.fieldRow }, [
        React.createElement("div", { style: styles.head }, [
          React.createElement("span", { style: styles.label }, t("dashboardBinaryTitle")),
          binaryBadge,
        ]),
        binaryDetail ? React.createElement("p", { style: styles.hint }, binaryDetail) : null,
      ]));

      // Login status row
      var login = s.login || {};
      var loginBadge;
      if (login.ok) {
        loginBadge = React.createElement("span", { key: "l", style: styles.tag }, t("dashboardLoginOk"));
      } else if (login.error) {
        loginBadge = React.createElement("span", { key: "l", style: styles.tag }, t("dashboardLoginError"));
      } else {
        loginBadge = React.createElement("span", { key: "l", style: styles.tag }, t("dashboardLoginOff"));
      }
      children.push(React.createElement("div", { key: "login", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement("div", { style: styles.head }, [
          React.createElement("span", { style: styles.label }, t("dashboardLoginTitle")),
          loginBadge,
        ]),
        login.error ? React.createElement("p", { style: styles.hint }, login.error) : null,
      ]));

      // Bridge status row
      var bridge = s.bridge || {};
      var bridgeBadge = React.createElement("span", { key: "br", style: styles.tag }, bridge.enabled ? t("dashboardBridgeOn") : t("dashboardBridgeOff"));
      var bridgeDetail = bridge.enabled ? t("dashboardPastedCount") + ": " + (bridge.pastedCount ?? 0) : "";
      children.push(React.createElement("div", { key: "bridge", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement("div", { style: styles.head }, [
          React.createElement("span", { style: styles.label }, t("dashboardBridgeTitle")),
          bridgeBadge,
        ]),
        bridgeDetail ? React.createElement("p", { style: styles.hint }, bridgeDetail) : null,
      ]));

      // Diagnostic actions row
      var actions = [
        { key: "reprobe", label: t("dashboardReprobe"), fn: function () { return remote.reprobeBinary(); } },
        { key: "relogin", label: t("dashboardRelogin"), fn: function () { return remote.relogin(); } },
        { key: "clean", label: t("dashboardClean"), fn: function () { return remote.cleanPasted(); } },
        { key: "selfCheck", label: t("dashboardSelfCheck"), fn: function () { return remote.selfCheck(); } },
      ];
      var actionButtons = actions.map(function (a) {
        return React.createElement("button", {
          key: a.key,
          type: "button",
          style: styles.trigger,
          disabled: snap.action === a.key,
          onClick: function () { runAction(a.key, a.fn); },
        }, snap.action === a.key ? t("dashboardLoading") : a.label);
      });
      children.push(React.createElement("div", { key: "actions", style: Object.assign({}, styles.fieldRow, styles.fieldDivider) }, [
        React.createElement("div", { style: styles.head }, [
          React.createElement("span", { style: styles.label }, t("dashboardActionsTitle")),
        ]),
        React.createElement("div", { style: { display: "flex", flexWrap: "wrap", gap: 8 } }, actionButtons),
        snap.actionError ? React.createElement("p", { style: styles.invalid }, t("dashboardError") + ": " + snap.actionError) : null,
      ]));

      return React.createElement("section", { style: styles.fieldRow }, children);
    }

    // Account pool section — renders the pooled accounts (label / status badge /
    // current & pinned markers) plus four actions: refresh health, add account,
    // remove account, pin/unpin account. Data comes from the host
    // VisionaryDashboard Remote (getAccounts / refreshAccounts / addAccount /
    // removeAccount / pinAccount) — no plugin-owned HTTP route.
    function AccountPoolSection(props) {
      var t = props.t;
      var remote = props.remote;
      var st = React.useState({ pool: null, error: null, loading: true, busy: null, notice: null });
      var snap = st[0];
      var setSnap = st[1];

      // 传方法名（而非取出的函数引用）——gateway 的 Remote 方法需要正确的 this，
      // 解绑后调用会丢上下文。
      // gateway 可能把 Remote 返回值包成 {ok:true,value} / {ok:false,error}，
      // 也可能直接给裸值 / reject——两种形状都要兼容。
      function unwrap(res) {
        if (res && typeof res === "object" && "ok" in res) {
          if (res.ok) return res.value;
          throw new Error(String(res.error ?? "remote call failed"));
        }
        return res;
      }

      // 所有 Remote 调用都经这里：绑定 this + 兼容 {ok,value} 包络。
      function remoteCall(method, arg) {
        return Promise.resolve()
          .then(function () { return arg === undefined ? remote[method]() : remote[method](arg); })
          .then(unwrap);
      }

      // `notice` 可选：添加账号是「启动」语义，启动提示要保留到用户下次刷新，
      // 不能被这次读列表立刻清掉。
      function load(method, notice) {
        setSnap(function (prev) { return Object.assign({}, prev, { loading: true, error: null }); });
        remoteCall(method || "getAccounts").then(
            function (data) { setSnap({ pool: data, error: null, loading: false, busy: null, notice: notice ?? null }); },
            function (err) { setSnap(function (prev) { return Object.assign({}, prev, { loading: false, error: String(err?.message ?? err), busy: null }); }); }
          );
      }

      React.useEffect(function () {
        // dashboard row 未启用时 remote 可能是 undefined——此时静默隐藏整块，
        // 绝不抛错（既有 dashboard 汇总也有同样的保护）。
        if (!remote) { setSnap(function (prev) { return Object.assign({}, prev, { loading: false }); }); return; }
        load();
      }, []);

      var children = [];

      children.push(React.createElement("div", { style: styles.head }, [
        React.createElement("span", { style: styles.label }, t("accountsTitle")),
      ]));

      if (!remote) return React.createElement("section", { style: styles.fieldRow }, children);

      if (snap.loading && !snap.pool) {
        children.push(React.createElement("p", { style: styles.hint }, t("accountsLoading")));
        return React.createElement("section", { style: styles.fieldRow }, children);
      }

      if (snap.error) {
        children.push(React.createElement("p", { style: styles.invalid }, t("accountsError") + ": " + snap.error));
        children.push(React.createElement("button", { key: "retry", type: "button", style: styles.trigger, onClick: function () { load(); } }, t("accountsRefresh")));
        return React.createElement("section", { style: styles.fieldRow }, children);
      }

      var pool = snap.pool || {};
      var accounts = Array.isArray(pool.accounts) ? pool.accounts : [];

      if (!pool.enabled || accounts.length === 0) {
        children.push(React.createElement("p", { style: styles.hint }, t("accountsEmpty")));
      } else {
        var summary = t("accountsSummary")
          .replace("{total}", String(pool.total ?? accounts.length))
          .replace("{healthy}", String(pool.healthy ?? 0))
          .replace("{muted}", String(pool.muted ?? 0))
          .replace("{expired}", String(pool.expired ?? 0));
        children.push(React.createElement("p", { style: styles.hint }, summary));
        if (pool.current) {
          children.push(React.createElement("p", { style: styles.hint }, t("accountsCurrent") + ": " + pool.current));
        }
        if (pool.env_ignored) {
          children.push(React.createElement("p", { style: styles.hint }, t("accountsEnvIgnored")));
        }
        accounts.forEach(function (a) {
          var badgeStyle = a.status === "healthy" ? styles.tag : (a.status === "muted" ? styles.tagWarning : styles.tagNeutral);
          var badgeText = a.status === "healthy" ? t("accountsHealthy") : (a.status === "muted" ? t("accountsMuted") : t("accountsExpired"));
          var row = [
            React.createElement("span", { key: "label", style: styles.label }, a.label || a.id),
            React.createElement("span", { key: "badge", style: badgeStyle }, badgeText),
          ];
          if (a.pinned || pool.pinned === a.id) {
            row.push(React.createElement("span", { key: "pin", style: styles.tagNeutral }, t("accountsPinned")));
          }
          children.push(React.createElement("div", { key: "acc-" + a.id, style: styles.head }, row));
        });
      }

      if (snap.notice) {
        children.push(React.createElement("p", { style: styles.hint }, snap.notice));
      }

      var poolActions = [
        { key: "refresh", label: t("accountsRefresh"), method: "refreshAccounts" },
        { key: "add", label: t("accountsAdd"), method: "addAccount" },
      ];
      var poolButtons = poolActions.map(function (a) {
        return React.createElement("button", {
          key: a.key,
          type: "button",
          style: styles.trigger,
          disabled: snap.busy === a.key,
          onClick: function () {
            setSnap(function (prev) { return Object.assign({}, prev, { busy: a.key, error: null }); });
            remoteCall(a.method).then(
              function (res) {
                var notice = a.key === "add" ? t("accountsAddStarted") : null;
                setSnap(function (prev) { return Object.assign({}, prev, { busy: null, notice: notice }); });
                // 不另造轮询机制：沿用既有 status 查询（dashboard 汇总 / 本区块刷新按钮）。
                load(a.key === "refresh" ? "refreshAccounts" : "getAccounts", notice);
              },
              function (err) { setSnap(function (prev) { return Object.assign({}, prev, { busy: null, error: String(err?.message ?? err) }); }); }
            );
          },
        }, snap.busy === a.key ? t("accountsLoading") : a.label);
      });

      accounts.forEach(function (a) {
        var pinned = a.pinned || pool.pinned === a.id;
        poolButtons.push(React.createElement("button", {
          key: "pin-" + a.id,
          type: "button",
          style: styles.trigger,
          disabled: snap.busy === "pin-" + a.id,
          onClick: function () {
            setSnap(function (prev) { return Object.assign({}, prev, { busy: "pin-" + a.id, error: null }); });
            remoteCall("pinAccount", pinned ? null : a.id).then(
              function () { setSnap(function (prev) { return Object.assign({}, prev, { busy: null }); }); load("getAccounts"); },
              function (err) { setSnap(function (prev) { return Object.assign({}, prev, { busy: null, error: String(err?.message ?? err) }); }); }
            );
          },
        }, pinned ? t("accountsUnpin") : t("accountsPin")));
        poolButtons.push(React.createElement("button", {
          key: "rm-" + a.id,
          type: "button",
          style: styles.trigger,
          disabled: snap.busy === "rm-" + a.id,
          onClick: function () {
            setSnap(function (prev) { return Object.assign({}, prev, { busy: "rm-" + a.id, error: null }); });
            remoteCall("removeAccount", a.id).then(
              function () { setSnap(function (prev) { return Object.assign({}, prev, { busy: null }); }); load("getAccounts"); },
              function (err) { setSnap(function (prev) { return Object.assign({}, prev, { busy: null, error: String(err?.message ?? err) }); }); }
            );
          },
        }, t("accountsRemove")));
      });

      children.push(React.createElement("div", { key: "accountsActions", style: { display: "flex", flexWrap: "wrap", gap: 8 } }, poolButtons));

      return React.createElement("section", { style: styles.fieldRow }, children);
    }
    function VisionaryBundleConfig(props) {
      return React.createElement("div", { key: "visionary-bundle-config" }, [
        React.createElement(VisionaryDashboardSummary, {
          key: "summary", t: props.t, remote: props.remote,
        }),
        React.createElement(AccountPoolSection, {
          key: "accounts", t: props.t, remote: props.remote,
        }),
        React.createElement(VisionaryCard, {
          key: "vision", t: props.t, view: "page",
          useVisionCard: props.useVisionCard,
          visionEdit: props.visionEdit, visionResetField: props.visionResetField,
          visionSave: props.visionSave, visionDiscard: props.visionDiscard,
        }),
        React.createElement(BridgeCard, {
          key: "bridge", t: props.t, view: "page",
          useBridgeCard: props.useBridgeCard,
          bridgeEdit: props.bridgeEdit, bridgeResetField: props.bridgeResetField,
          bridgeSave: props.bridgeSave, bridgeDiscard: props.bridgeDiscard,
          bridgeTriggerClean: props.bridgeTriggerClean,
        }),
      ]);
    }

    // ── apply ─────────────────────────────────────────────────────────────────

    var inject = ["slots", "locale", "configForms", "remote"];

    function apply(ctx) {
      ctx.effect(function () {
        return ctx.locale.register(NS, { zh: LOCALE_ZH, en: LOCALE_EN });
      }, "locale: " + NS);
      var t = ctx.locale.bind(NS);

      var vision = makeVisionCard(ctx, t);
      var bridge = makeBridgeCard(ctx, t);
      ctx.effect(function () {
        return function () {
          vision.dispose();
          bridge.dispose();
        };
      }, "visionary: form subscriptions");

      // Bundle configuration: a single `plugins.bundle.config` registration
      // (keyed by bundle package name) renders on the `@xlight-oss/visionary-dsh`
      // bundle detail page between its description and rows. The composite
      // `VisionaryBundleConfig` component renders the dashboard summary plus the
      // Vision Tools and Image Bridge forms inline. The combined `inject()`
      // merges the namespaced faces from `vision.inject()` and `bridge.inject()`
      // with the dashboard remote, per design D3.
      ctx.effect(function () {
        return ctx.configForms.whileServed([NS_VISION, NS_BRIDGE], function () {
          var off = ctx.slots.inject("plugins.bundle.config", function () {
            return ctx.slots.register({
              name: "plugins.bundle.config",
              key: "@xlight-oss/visionary-dsh",
              order: 10,
              label: function () { return t("dashboardSummary"); },
              locale: NS,
              inject: function () {
                var vf = vision.inject();
                var bf = bridge.inject();
                return Object.assign({ remote: ctx.remote.visionaryDashboard }, vf, bf, {
                  hooks: Object.assign({}, vf.hooks, bf.hooks),
                });
              },
            }, VisionaryBundleConfig);
          });
          return function () { off(); };
        });
      }, "visionary: bundle config");
    }

    exports.inject = inject;
    exports.apply = apply;
    return module.exports;
  }
});
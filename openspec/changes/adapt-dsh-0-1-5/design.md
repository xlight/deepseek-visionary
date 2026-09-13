# adapt-dsh-0-1-5 — 设计

## Context

见 `proposal.md` 的 Why。设计只补齐「怎么改」所需的现状与约束：

- 运行面：本机 DSH 为 0.1.5（GUI/agent 进程 0.1.5-rc.2；CLI `dsh --version` = 0.1.5-rc.1），npm 上 `latest` = 0.1.5-rc.1、`next` = 0.1.5-rc.2。插件声明的 `^0.1.0-rc.6` 按 semver 预发布规则实测只解析到 `0.1.0-rc.8`（`npm view "@deepseek-ai/dsh-tools@^0.1.0-rc.6" version` → 0.1.0-rc.6/7/8）。
- 上游契约现状（0.1.5-rc.2 实测，路径为 `.pnpm-store/.../links/@deepseek-ai/<pkg>/0.1.5-rc.2/...`）：
  - `dsh-settings` 导出仅 `SettingsConflictError / SettingsProvider / redactSecrets`；`SettingsProvider.installSection(owner, ns, schema, entry, hooks)` 位于 `lib/index.js:327`，`hooks` 只读 `validate`（可选）、`setSource(thunk)`、`onChange()`（安装时同步调用一次），注册为 fiber effect，provider 脱离时回退 `setSource(() => entry)`；命名空间由 `/^[a-z][a-z0-9-]*$/` 校验（`lib/index.js:82`）。
  - 官方消费者（`dsh-bash-local`、`dsh-pwsh-local`、`dsh-agent-default-model`、`dsh-web-search-deepseek`）统一写法：`ctx.inject(["settings"], (settingsCtx) => settingsCtx.settings.installSection(ctx, NS, Schema, entry, hooks))`，`owner` 是**外层**插件 `ctx`。
  - `dsh-client-modules`：browser resolution 顺序为 seed 词 → 已物化模块 → boot graph 行 → 已注册工厂（`lib/client.js:303-308`）；`dsh.client.inject` 在 `arriveGraphRow` 中对存在的行做“先到达依赖再到达自己”（`lib/client.js:265-268`），缺席的行被静默跳过；`dsh.client.external` 才参与 `orderByModuleGraph` 的缺供应商校验。平台种子表由 shell（`dsh-web-frontend` 产物）注入 `staticModules`，其中含 `@deepseek-ai/dsh-client-store`，**不含** `@deepseek-ai/dsh-client-runtime`（该包 0.1.2-alpha 起被删，npm 上仅存 0.0.1-rc.1）。
  - 客户端设置面：`ctx.settingsScope.bind({ namespace, decode? })` → `SettingsScope<T>{ getSnapshot, subscribe, set(field,value), unset(field), mutate(ops, expectedRevision?) }`，快照含 `status/value/base/user/revision/writable/mode`（`dsh-client-ui-settings/lib/types/client/settings-contract.d.ts`）；插件配置卡槽 `settings.plugin.item`（`kind: keyed`，options `key` = 命名空间）由 `dsh-client-ui-settings-plugins` 在运行时声明，其类型文档明确该槽位就是为**仓库外插件**设计的（"registers its own settings namespace on the Host and its own card under that key in the browser"）。Settings → Plugins → Plugin configuration 只渲染「宿主已服务的命名空间 ∩ 已注册卡片」的交集，一张卡都不注册就什么都不显示；`dsh-client-schema-form` 已在 0.1.0-rc.8 移除，没有通用 schema 表单可用。
  - `dsh-api-settings-controller/lib/index.js:429`：`settings.describe({ redactSecrets: true })` 返回**每个**已注册命名空间，不再有第三方白名单。
  - 图片桥接：admission gate 仍是硬编码 `MODEL_DOES_NOT_SUPPORT_IMAGES`（`dsh-api-session-controller/lib/index.js:763-764` 与 `dsh-host-apiproxy/lib/index.js:2755-2758` 同一字面量与 `reason` 码），只读 `ctx.llm.resolveModelInfo`，**无 waterfall、无事件、无插件扩展点**；新版文本模型占位符为 `[image omitted because this model accepts text only; attachment sha256:…]`（`dsh-llm/lib/content.js:47-50`）**不含路径**。故「不改 admission 就能拿到图片」在本版本不成立，但「只有补丁一条路」也不成立——见社区两条路线（Context 普查末条）与 D7。`ctx.imageRouting` / `resolveFallback` 在 0.1.5-rc.2 全树零命中；该服务只在**一个未发布到 npm 的仓库**（`CuzWeAre/dsh-llm-image-routing`，1 star）里被提出，240 个 `@deepseek-ai/*` 包与已抽样社区插件中零消费，且该仓库自身还导入了不存在的 `installSettingsSection`/`settingsNamespace`，在任何已发布宿主上都加载不了。`llm.registerAdapter` 确实存在于 0.1.5（`dsh-llm/lib/index.js:1780`），但对已注册的 provider 路由会抛 `DUPLICATE_ADAPTER`（`:1810`），所以社区做法只能注册**合成的**包装 provider，需要用户改选模型。
  - 官方实现细节（支撑 D2）：`installSection(owner, ns, schema, entry, hooks)` 内部就是 `register(ns, schema, { base: entry, validate })` + `setSource(() => scope.get())` + 一次性 `onChange()` + `scope.watch(onChange)` + 卸载时回退 `setSource(() => entry)`（`dsh-settings/lib/index.js:281-343`）；它**不返回** scope，读取只能经 hooks。`describe()` 还返回 `schema.toJSON()` 与 `base`/`user` 两层原始值（`lib/index.js:344-366`），供配置面标注「用户已覆盖」与「重置后回到什么」。
  - 社区参考实现（同机已安装、均已读源码；n=2）：
    - `dsh-better-sidebar@0.19.1`（peer `^0.1.5-rc.1`、dev 精确固定 `0.1.5-rc.2`，`package.json:75-159`——peer caret 下限 + dev 精确锁定的版本约定来源）：宿主侧 `ctx.inject(["settings"], (sctx) => sctx.settings.register(ns, PrefsSchema))`（2 参、无 `base`，`src/index.ts:801-809`），浏览器侧**不用** `settingsScope`，而是自建 fenced 路由 `POST /sidebar/api/settings.get|update`（`src/trust-fence.ts` 复制 `/api` 网关栅栏，`src/index.ts:884-913`）。其源码注释（`src/index.ts:766-773`）称「DSH settings RPC 只服务白名单命名空间」——**该说法在 0.1.5-rc.1 已不成立**（见下方 `dsh-api-settings-controller` 一条），故本项目不沿用该绕过。
    - `dsh-pocket@2.10.6`：**完全不注册** settings 命名空间，配置存 `$DSH_HOME/dsh-pocket/settings.json`，经私有 `connection.rpc.call("/dsh-pocket", …)` 通道读写，无修订栅栏（`lib/settings.mjs:20-65`、`lib/web-rpc.js:215-232`）。
    - 两者都注册 `settings.section` 自有整页、**都不注册** `settings.plugin.item` 卡；原因是其界面为 schema 表单装不下的定制内容（页签/查看器清单、二维码与隧道生命周期）。本项目两个命名空间都是小表单，故不跟随此惯例（见 D3）。
    - `dsh.client` 字段语义：二者都只声明 `platform` + `inject`，均未用 `external`；`inject` 边为加载/预取元数据（`dsh-client-ui-workspace/lib/client.js:2703-2709`「informational, never apply sequencing」）。
  - 社区生态普查（2026-09-13 于 npm registry + 已安装树，样本 30 个已发布插件；供下面各决策定标）：
    - **设置面落点**：`settings.section` 自有整页最多（16 个明确注册中 9 个），`settings.plugin.item` 4 个、`settings.plugins.tab` 3 个。`settings.plugin.item` 有真实先例且官方文档明确面向仓库外插件（`dsh-client-ui-settings-plugins/lib/types/client/slot-contract.d.ts`：*「Keying on the namespace is what lets a plugin distributed outside this repository contribute a card」*；官方 `docs/cookbook/adding-a-settings-card.md`：*「Nothing in this path needs a change inside this repository」*）；先例：`@liustack/modlens@3.26.1`（`dsh/client.js:1118`）、`dsh-plugin-model-proxy@0.1.3`（`lib/client.js:1569`）。
    - **私有 host 路由是社区常态**，不是被劝阻的做法：`@linxin666/dsh-tool-describe-image` 挂 `/describe-image/*`（`lib/index.js:1794`）、`@linxin666/dsh-client-ui-web-ui-settings` 挂 `/api/dsh-web-ui-settings/describe|mutate`（`lib/index.js:455`）。社区指南要求的是「挂在已文档化的扩展点」，不是「不许有自有路由」。
    - **版本范围实测**（semver 7.8.5）：预发布版本只有在比较符携带**同一 `[major,minor,patch]` 元组且自身是预发布**时才可被命中。故 `^0.1.0-rc.6`、`>=0.1.0-rc.6`、`<0.2.0`、`<0.2.0-0`、`*` 对 `0.1.5-rc.1/rc.2` **全部不匹配**（`<0.2.0` 正是被广泛引用的社区建议，实为无效）；可用的写法是 `^0.1.5-rc.1`、`>=0.1.5-rc.1 <0.2.0`，或像 `dsh-plugin-guide` 那样显式列两个元组的 `||` 链。未被列出的未来元组（如 `0.1.6-rc.1`）仍会被 `^0.1.5-rc.1` 排除。
    - **客户端 bundle**：`window.__ModuleLoader__.load({ id, factory })` 普遍（30/30 声明 `exports["./client"]`）；产物工具以 tsdown 13/30、esbuild 5/30 为主，12/30 无打包器（自写脚本），**手写该格式被官方文档认可**（官方 cookbook 明言「a package outside this repository has to reproduce the same output format itself」，`@liustack/modlens` 即手写）。
    - **社区「未知 API 风险」做法以特性探测为主**（`@liustack/modlens` 连 DSH peer 都不声明，通篇 `typeof llm?.registerAdapter !== 'function'` 式守卫；`describe-image` 对 `installSection` 做 `register` 回退）。本项目不采用无声明路线（见 D1），但同一 change 内的运行时检查与之同向。
    - **图片桥接的社区路线**（见 D7 与上文 admission gate 一条）由 `@liustack/modlens@3.26.1` 验证：不改 admission，改为 `llm.registerAdapter([合成的 provider id], …)` 注册一个把 `inputModalities` 加上 `image` 的包装 provider（`dsh/index.js:753-759`，只**调用** `resolveModelInfo`、不补丁），用户在模型选择器里选 `(modlens vision)` 变体；Web 端另有浏览器半在粘贴时把图片转成文件路径文本，使消息不含 attachment（*「Admission never fires because the message carries no image attachment」*）。
- 现有实现的耦合点：`lib/index.mjs:17,28,300`、`lib/image-bridge/index.mjs:33,57,208`（settings 注册）、`lib/settings-card/client.js:8,160-234`（模块 id + fetch 私有路由 + 自建快照世代）、`lib/settings-route.mjs`（挂载 `/visionary/api`）。

## Goals / Non-Goals

**Goals:**

- 插件在 DSH 0.1.5（rc.1 / rc.2）上完整可用：5 个原生工具、图片桥接、原生设置页配置。
- 配置面收敛到 DSH 原生设置通道，删除插件自建的平行机制（私有 HTTP 路由 + 自写快照同步）。
- 依赖声明与实际契约一致，并让 CI 能在下次上游变更时立刻发现漂移。

**Non-Goals:**

- 不改变工具面、CLI 参数面、Rust 二进制契约与 `COMPAT_MINOR = 0.7`。
- 不改变图片桥接语义（admission patch、`llm/stream` 改写、落盘/TTL、agentic/deterministic 模式、不可信内容框定）。
- 不为 DSH 0.1.0-rc.x 保留兼容分支（见 D1）。
- 不重命名插件行 id 与设置命名空间（保持既有 `cordis.patch.yml` / `settings.yaml` 兼容）。

## Decisions

### D1：只支持 0.1.5 线，不做双版本兼容

`peerDependencies` 升到 `^0.1.5-rc.1`（caret 下限，覆盖 rc.1、rc.2），`devDependencies` 精确锁定当前基线 `0.1.5-rc.2`（社区做法：peer 下限留在基线之前，好让 rc.1 宿主仍可安装），`@deepseek-ai/cordis` → `^4.0.2`，`@deepseek-ai/schemastery` → `^3.18.2`。

- 理由：被移除的 `installSettingsSection` / `settingsNamespace` 与 `@deepseek-ai/dsh-client-runtime` 都不是可选面，双支持需要运行期探测（settings 句柄形状 + 模块 id 试探）与两套客户端 bundle，复杂度远超收益；插件经 npm 分发、可按宿主版本固定。
- 备选：dynamic import 探测旧 API 并回退 → 弃用；两套客户端 bundle（按 `dsh.client` 声明切换）→ 弃用。
- 注：`^0.1.5-rc.1` 允许 0.1.5 的 rc.2（同 [major,minor,patch] 元组含预发布），与参考插件一致；`dsh-client-store` 作为 seed 词**不需要**写入 `dsh.client.inject`（浏览器解析在 seed 分支即命中），沿用其注释所述事实。

### D2：settings 注册改为 `installSection` + inject-wait

```js
const SETTINGS_NAMESPACE = "visionary-vision"           // 字面量，替换 settingsNamespace()
ctx.inject(["settings"], (settingsCtx) => {
  settingsCtx.settings.installSection(ctx, SETTINGS_NAMESPACE, Config, config, {
    setSource: (thunk) => { source = thunk },
    onChange: () => { runtime = { ...source() } },       // 安装时会被同步调用一次
    validate: validateConfig,                            // 仅桥接需要
  })
})
```

- 理由：hooks 形状与旧 helper 完全一致（`setSource` / `onChange` / `validate`），迁移是机械替换；inject-wait 天然实现「无 settings provider 时用 entry 配置」，与官方消费者写法一致。读上游实现可确认这正是官方为「设置即 composition entry」准备的那条路径：`installSection` = `register(ns, schema, { base: entry })` + 卸载回退（`dsh-settings/lib/index.js:327-343`）。
- 影响：`runtime` 的初始化必须在注册前保持 `{ ...config }`，因为 `onChange` 会在 `apply` 过程中同步触发一次（现有代码已是该形状）。`installSection` **无返回值**（不返回 scope），读取一律经 `setSource` / `onChange`，现有实现已是该形状。
- 备选：直接 `ctx.settings.register(ns, Config, { base: config })` + 自管 watch → 弃用。这是社区插件 `dsh-better-sidebar` 的写法（`src/index.ts:801-809`，2 参无 `base`），适用于「设置是独立文档、与行配置无关」的场景；本项目需要 provider 脱离后回退到行配置，自管该逻辑等于重写 `installSection`。

### D3：配置面迁移到原生 Plugins 设置页，删除私有路由

- 删除 `lib/settings-route.mjs` 与 `lib/settings-card` 中的 `callVisionaryApi` / 自建快照世代层；连同只被该路由使用的 `lib/image-bridge/trust-fence.mjs` 及其单测一并删除（无路由即无需要守护的私有端点）。
- **删除** `cordis.patch.yml` 的 `visionary-settings-card` 行与嵌套子包 `lib/settings-card/{index.mjs,package.json}`。浏览器 bundle 的发现契约是**裸包名**（见 D9）：主行 `visionary-vision`（`name: '@xlight-oss/visionary-dsh'`）已经提供了发现入口，子路径行名反而拿不到任何 bundle。`dsh.client` + `exports["./client"]` 因此上移到主 `package.json`，浏览器半落到 `lib/client.js`。
- 客户端卡片注册到 `settings.plugin.item`（options `key` = 命名空间），每个命名空间一张卡，出现在 Settings → Plugins → Plugin configuration：
  - `visionary-vision`：主区 `binaryPath` / `modelType` / `visionTimeoutMs`；折叠的「高级」区 `statusTimeoutMs` / `loginTimeoutSeconds`
  - `visionary-image-bridge`：主区 `enabled` / `scope` / `mode`；折叠的「高级」区 `promptTemplate` / `pastedDir` / `retainHours` / `cleanPasted`（一次性触发器）
  - 折叠状态是组件本地状态、不写入 settings 文档；字段集合与顺序沿用旧卡片，只改分组与数据通路。
- 卡片可用快照的 `user` / `base` 层标注「已覆盖（重置回到 base）」：重置走 `unset(field)`，这是原生通道顺带获得的能力，旧实现没有。
- 读写经 `ctx.settingsScope.bind({ namespace })`：`getSnapshot()` 驱动渲染，`subscribe()` 触发重绘，保存时以读取到的 `revision` 作为栅栏提交 `mutate(ops)`（`{op:'set', path:[field], value}` / `{op:'unset', path:[field]}`）；冲突由原生通道拒绝，卡片重读。
- `binaryPath` 现由工具与桥接两个命名空间共享（旧 client 的 `sharedField` 同时写两处）。选择：由「Visionary」卡绑定两个 scope，保存 `binaryPath` 时同时写 `visionary-vision.binaryPath` 与 `visionary-image-bridge.binaryPath`，保持既有语义与用户心智。备选：两张卡各持一份（用户需设两次）；合并为单一命名空间（破坏既有 `settings.yaml` 键与 patch 兼容）——均弃用。
- `cleanPasted` 一次性触发器保留：卡片写入 `true`，桥接 `onChange` 清理并把运行态与持久化值复位为 `false`（现有逻辑不变）；它放在桥接卡的折叠区，避免与常规字段混在一起被误触。
- 理由：原生通道已服务所有命名空间并提供修订栅栏，自建路由的 403/409/白名单理由已消失；卡片仍必须自写 UI（无通用 schema 表单）。`settings.plugin.item` 是上游明确为**仓库外插件**设计的落点，官方文档与真实先例都在（`@liustack/modlens`、`dsh-plugin-model-proxy`）；社区整体仍以 `settings.section` 整页居多（16 个注册中 9 个），本项目两个命名空间都是标量/短文本，用按命名空间 keyed 的卡片即可把配置留在原生的 Plugins 组织方式里。删除私有路由是**主动收敛**（少维护一条平行通道与自写快照层），不是「私有路由不合规」——社区里 fenced 自有路由是常态（`/describe-image/*`、`/api/dsh-web-ui-settings/*`）。
- 备选：`settings.plugins.tab` 自建页签（保留旧「合并页面 + 共享 binaryPath」布局，社区 `dsh-vision-bridge` 的做法）→ 多一个自有页面，收益仅是布局连续；`settings.section` 自有整页（社区两个插件的做法，diff 最小）→ 配置跑到 Plugins 之外，与本次「收敛到原生通道」的目标冲突；保留私有路由做最小迁移 → 已选原生路线。三者均弃用，定稿 `settings.plugin.item` 两张卡。

### D4：客户端 bundle 只依赖当前 shell 提供的模块

- **删除** `require("@deepseek-ai/dsh-client-runtime/client")`：它在旧 `client.js` 里只服务 `createSnapshotStore`（`:8` 引入、`:184` 唯一调用），而新卡片以 `settingsScope.getSnapshot()/subscribe()` + React 本地状态驱动渲染，快照世代层整体消失 → 该模块边彻底去掉（不是「换成 `@deepseek-ai/dsh-client-store`」；0.1.5 树内 `dsh-client-runtime` 已整体不存在，seed 表里的 `@deepseek-ai/dsh-client-store` 当前用不上）。
- 主 `packages/dsh-plugin/package.json` 的 `dsh.client.inject`：去掉 `@deepseek-ai/dsh-client-runtime`，列 `@deepseek-ai/dsh-client-locale`、`@deepseek-ai/dsh-client-ui-settings`（`settingsScope` 提供方）、`@deepseek-ai/dsh-client-ui-settings-plugins`（`settings.plugin.item` 槽位声明方）。该字段只是加载/预取元数据（不参与 `orderByModuleGraph`，只有 `external` 参与，缺席行被静默跳过），真正的等待靠客户端插件的 cordis `inject: ["slots", "locale", "settingsScope"]`；已核实三者在本机 0.1.5-rc.2 树内确实由 `dsh-client-ui-renderer`（`super(ctx, "slots")`）、`dsh-client-locale`（`ctx.provide("locale", …)`）、`dsh-client-ui-settings`（`super(ctx, "settingsScope")`）提供，故服务等待可满足、不会悬停不激活。社区两种做法都存在（`@liustack/modlens` 声明空 `inject` 也照常注册卡片，`dsh-plugin-model-proxy` 声明 `@deepseek-ai/dsh-client-ui-settings-plugins`），故这一条是**声明性**的，不影响可用性。
- 重写后卡片的模块边只有两条：`react` 与 `@deepseek-ai/dsh-client-ui-primitives`（宿主的平台 seed，**不是**某个插件行）。后者只为一个控件：布尔字段的 `Switch`（D10）。宿主自带卡片无条件 require 同一个 seed（`dsh-client-ui-settings-plugins/lib/client.js` 里 `require("@deepseek-ai/dsh-client-ui-primitives")`，其 `dsh.client.inject` 也并未声明它，因为它属基线），故这条边不增加发现风险；`require` 外包一层 try/catch，seed 缺席时退回原生 checkbox 而不是让整张卡物化失败。
- bundle 仍以 `window.__ModuleLoader__.load({ id, factory })` 手写交付；`exports["./client"]` → `lib/client.js`，注册 id 必须等于**包名** `@xlight-oss/visionary-dsh`（D9：runner 以包名 require 该行来激活它，id 用嵌套包名会得到 `no registered factory`）。
- 理由：模块解析失败是本次硬故障之一，减少模块边即减少漂移面；社区两个插件的浏览器半也只 require `react`（+ 可选 primitives），未声明 `external`。产物格式本身是约定而非官方发布物：`window.__ModuleLoader__.load({ id, factory })` 是通用形状（抽样 30/30 声明 `exports["./client"]`），官方 cookbook 明确要求仓库外包自行「reproduce the same output format」，手写合法（`@liustack/modlens` 即手写），故本插件继续手写交付无需引入打包器。

### D5：测试与 CI 让漂移可见

- 更新 `test/tools.test.mjs` / `test/integration-smoke.test.mjs` 的伪 settings 服务为 `installSection` 契约（含「无 settings 服务 → 回退 entry」与「onChange 热重载」断言）；保留既有的 dependency-free 单测。
- `ci.yml` 增加插件作业：`pnpm install --frozen-lockfile` + `node --test`（工作目录 `packages/dsh-plugin`）。理由：现有 CI 只跑 cargo，插件测试从未执行，本次漂移因此未被拦截。

### D6：版本与发布

- `scripts/bump_version.py` **移除**对 `packages/dsh-plugin/lib/settings-card/package.json` 的同步（该嵌套包已删除，一致性校验回到 7 个条目：Cargo.toml / Cargo.lock×2 / extension.toml / package.json / COMPAT_MINOR / server.json）。
- 本次随 `0.7.3` 发布（Rust 工作区、Zed 扩展、npm 插件单版本策略）；`pnpm-lock.yaml` 必须随提交更新，否则 `dsh-plugin-release.yml` 的 `--frozen-lockfile` 失败。

### D7：图片桥接的 admission 处理保持「补丁 + `llm/stream` 改写」，删除 `imageRouting` 死分支

- 保持：`lib/image-bridge/core.mjs` 的 `makeModelInfoPatch`（`ctx.effect` 安装/还原）仍是本插件在 0.1.5 上放行图片的**透明**手段——对所有 provider / 模型生效，用户无需改选模型；`llm/stream` 改写继续承担「图片换成路径 + 文本引导」。
- 备选（社区已验证、本次不采用）：modlens 的 `llm.registerAdapter([合成 provider id], …)` 包装 provider，把 `inputModalities` 补上 `image`，让 gate 自行放行（`dsh/index.js:753-759`）。它落在官方 seam 上、不补丁任何方法，代价是必须注册**新的** provider 路由（同名路由抛 `DUPLICATE_ADAPTER`，`dsh-llm/lib/index.js:1810`）、用户要在模型选择器里改选 `(modlens vision)` 变体，作者还要自己处理发现、自嵌套与重入（modlens 为此写了 reconcile 循环 + `llm/adapters-updated` 监听）。与「装上就生效」的定位不符，记为后续 change 候选。
- 备选（不采用）：浏览器半粘贴接管（modlens 的 paste→路径路线，让消息干脆不含 attachment）。需要客户端钩住 composer 的粘贴/附件入口，本插件目前没有这层，收益只是绕开 admission 而非绕开改写。
- **删除** `ctx.imageRouting` 前向兼容块及其注释：该契约只出现在一个未发布的仓库里、无任何已发布宿主消费（见 Context），而注释称其为「社区咨询契约」，属误导；保留只会多一条永不执行的路径。删除不影响补丁与 `llm/stream`（该分支原本只在 `ctx.get("imageRouting") !== undefined` 时跳过补丁）。
- 补丁健壮性：`makeModelInfoPatch` 现在直接 `llm.resolveModelInfo.bind(llm)`，方法缺失会在 `apply` 期抛错并拖死整个插件行；改为安装前检查 `typeof ctx.llm?.resolveModelInfo === "function"`，缺失时记 warn 并按「桥接关闭」继续。与社区的特性探测做法同向（Context 普查）。

### D9：浏览器 bundle 的发现契约要求**裸包名行**（spike 实测结论）

- **机制**（`@deepseek-ai/dsh-client-modules@0.1.5-rc.2`，宿主半 `lib/index.js`）：扫描以 Loader 行的 `options.name` 为键（`processOne(entryName)` 只匹配 `entry.options.name === entryName`），经 `resolveMeta` → `locatePkgJson(loaderName, baseUrl)`。而 `locatePkgJson` 第一件事是 `pathLike ? undefined : exactPackageSpecifier(loaderName)`，其中 `exactPackageSpecifier`（`:132-138`）**只接受裸包根**：`@scope/name` 若切分后不是恰好两段，或非 scoped 名里含 `/`，一律返回 `undefined` —— 随后 `if (!pathLike && expectedPackageName === void 0) return void 0`（`:683`）**直接放弃，不做任何解析**。子路径行名因此既不报错也不留痕：`resolveMeta` 缓存 `null`，该包永远不会被 `reconcilePackage` 建成 boot graph 行。
- **实测**（本机 0.1.5-rc.2 真实 `dsh web`，`D:\Temp\dsh-scratch` 隔离 profile，与 GUI 同构）：旧结构（`visionary-settings-card` 行 + 嵌套包 `dsh.client`）下，`GET /` 的 `window.__DSH_BOOT__` **完全不含** `@xlight-oss/visionary-dsh/client.js`，启动 combo 里也没有它 —— 浏览器半根本没下发，这正是用户报的「两张卡不出现」。改为裸包名行 + 主 manifest 声明 `dsh.client` 后，boot graph 出现 `{"id":"@xlight-oss/visionary-dsh","url":"/plugins/??@xlight-oss/visionary-dsh/client.js&rev=…"}`，该 URL 返回 **200 / 30753 字节**（即 `lib/client.js` 本体）。
- **第二半契约**：graph 行的请求 id 是 `<pkg>/client.js`，但 runner 激活该行时是按**包名** `require` 工厂（`graphRow(packageName, …)`，`:329-337`；client 半 `no registered factory for "<id>"`，`:277`）。所以 bundle 内 `window.__ModuleLoader__.load({ id })` 必须写包名 `@xlight-oss/visionary-dsh`，写嵌套名会在物化期抛 `no registered factory`。
- **浏览器级闭环**（同一验证机上用 CDP 驱动无头 Chrome，真实 `dsh web` + 真实设置面板）：Settings → Plugins → 插件配置 渲染出「Visionary · 视觉工具」「Visionary · 图片桥接」两张卡；卡片显示的是组合 entry 的生效值（`300000/60000/600`）；改 `visionary-vision.visionTimeoutMs` → 保存 → 宿主 `settings.yaml` 写入 `240000`；该字段随后带「已覆盖」标记与「重置」，重置后 `settings.yaml` 中该键被 `unset` 移除。整页 reload（`ignoreCache`）期间 `Runtime.exceptionThrown` + `Log.entryAdded` 捕获到 **0 条错误**。即：卡片能渲染、能写、能重置，且 `settingsScope` 的原生通道与修订栅栏均按预期工作。
- **落地**：`dsh.client` + `exports["./client"]` → `./lib/client.js` 写在主 `package.json`；`cordis.patch.yml` 只留两个行（`@xlight-oss/visionary-dsh`、`.../image-bridge`，后者无 `dsh.client`、保持惰性即可）；删除 `lib/settings-card/{index.mjs,package.json}` 与 `exports["./settings-card"]`。`test/integration-smoke.test.mjs` 新增 `client discovery` 用例把这三条钉死（裸包名行存在、子路径行不可承载 client、`exports["./client"]` 指向存在文件、注册 id == 包名）。
- **对社区缺陷报告的修正**：Discussion #1470「Configurable plugins tab renders empty」与 modlens issue #61/#65 属**派发侧**问题；本次实测中派发侧是好的（`ConfigurablePluginsTab` 渲染 `describe().namespaces ∩ slots.entries("settings.plugin.item").key`，`lib/client.js:20636-20650`、`tab-store` 控制器），空标签页由**发现侧**（本插件自己的行名）造成。故不退回 `settings.plugins.tab`，`settings.plugin.item` 落点维持不变。

### D10：卡片外观对齐宿主自己的 `PluginCard`（实现期修正）

- **问题（用户实测反馈）**：卡片能渲染、能读写，但「样式不太对」。根因是首版卡片用 inline style 手搓了一套近似的 chrome：标题在卡片**外面**（`<div>` + `<h3>`/`<p>`），卡片永远展开、没有折叠箭头，边框 `1px border-l2`／圆角 `12px`（宿主是 `.5px border-l4`／`16px`），输入框没有焦点态与 34px 高度，保存按钮是 `--dsw-alias-brand-primary` 蓝底白字（宿主是 `label-primary` 底 + `bg-layer-3` 字），布尔字段渲染成浏览器默认 13px checkbox。
- **对照取证**（宿主自带的 `dsh-client-ui-settings-plugins` 客户端 bundle）：卡片 = `<li class="…card">` + 折叠 `<button class="…header">`（名字 15px/600、描述 13px、右侧 14px chevron、dirty 时插一个 neutral Tag「未保存」）+ `…cardOpen` 时 `<div class="…body">`（`border-top:.5px border-l2; margin:0 16px; padding-bottom:8px`）；字段 = `…field`（`padding:12px 0; gap:6px`，相邻字段之间 `.5px` 分隔线）+ `label`（13px/500）+ `…input`（`height:34px; padding:0 12px; radius:8px; border:.5px border-l4`，`:focus-visible` 换 `brand-primary` 边框并去掉 outline）+ `…hint`/`…invalid`（12px）；底部 = `…footer` + `…discard`／`…save`（save = `label-primary` 底 + `bg-layer-3` 字，`disabled` 时 `opacity:.4`）；覆盖态 = `Tag(tone:"neutral")`「已覆盖」+ 文本按钮「恢复默认」。保存成功（`!dirty && !failed`）后卡片自动收起。
- **做法**：不 require 宿主那个包（它的 CSS-module 类名按构建哈希，`YyYd_a_card` 之类引用出去必碎），而是把上述规则**逐条镜像**到自己注入的一张 `<style data-plugin="@xlight-oss/visionary-dsh">` 里（类前缀 `vlb-`，同一组 `--dsw-alias-*` token）；结构与文案同步对齐（`<li>` 折叠行、`aria-expanded`/`aria-label="展开设置: …"`、dirty Tag、`.5px` 分隔线、`恢复默认`、保存后收起）。控件层面只借宿主的 `Switch`（见 D4），chevron 用同一条 path 内联 SVG（少一条模块边）。
- **验证**（真实宿主 + 无头 Chrome + CDP）：`getComputedStyle` 逐项对比我们与宿主原生的 `li`/header/name/description/body/field/label/input/hint/save/discard/footer/chevron —— 除文本行数造成的自然高度差外**逐项一致**；布尔字段渲染为宿主 `button[role="switch"]`（`_switch_*` 类）而非 checkbox；点击开关 → 头部出现「未保存」→ `保存` 可点 → 宿主 `settings.yaml` 写入 `visionary-image-bridge.enabled: false` → 卡片自动收起。单测同步改为按 `vlb-*` 类名断言，并新增折叠/aria/dirty/Switch/样式注入与卸载等用例。
- **取舍**：镜像规则会随宿主改版出现观感漂移（不影响功能，最坏是某个颜色回退到旧 token）；替代方案是 require 宿主包直接复用其组件，但该包只导出 `apply`，卡片 chrome 并未对外导出，且哈希类名不可引用。故选择镜像 + 逐项取证。

## Risks / Trade-offs

- [旧 DSH 用户升级插件后失效] → 插件 README 与接入文档明确「本版本要求 DSH ≥ 0.1.5-rc.1」；不支持的组合在 npm 上仍可安装旧版本 0.7.2。
- [原生 Plugins 页只列 host-plane 插件命名空间] → 安装路径本就是 `dsh plugin --profile web add`（host 平面），文档同时说明该前提；若被塞进 agent preset 则配置页不出现（与上游行为一致）。
- [两个命名空间必须各注册一张卡，漏一张会出现只有标题的空卡] → tasks 中把「两张卡都注册」列为验收项；冒烟测试断言 `settings.plugin.item` 有两个 key。
- [社区惯例与本次落点不同]（抽样里 `settings.section` 整页占多数 9:4；`dsh-better-sidebar`、`dsh-pocket` 都走自有整页 + 私有 host 通道，都不用 `settings.plugin.item`）→ 该槽位是上游明确为仓库外插件设计的落点，有真实先例（`@liustack/modlens`、`dsh-plugin-model-proxy`）与官方 cookbook 背书，风险限于「观感与社区最常见的第三个整页不同」，不影响功能。
- [`settings.plugin.item` 在个别 0.1.5-rc.x 构建上可能渲染不出内容]（社区存在真实字段缺陷：讨论 #1470「Configurable plugins tab renders empty」，modlens 自带 issue #61/#65 与其注释提到的「since rc.7」派发变更；且卡片只在宿主服务同一命名空间时渲染）→ 实现期**先做渲染 spike**：在本机 0.1.5-rc.2 上以真实 `dsh web` 读取 boot graph 验证。**spike 结论（已执行）**：派发侧正常，缺卡的真因是本插件的 bundle 未被下发（子路径行名，见 D9），修复后 bundle 200 下发；故落点维持 `settings.plugin.item`，不回退 `settings.plugins.tab`。命名空间由 `installSection` 注册，故「宿主未服务该 key」这一条对本插件不成立。
- [`llm.resolveModelInfo` 猴补丁在新版 HMR/invariant 下的稳定性未验证] → 见 D7：安装前加 `typeof` 检查并降级、`ctx.effect` 还原、社区替代路线（adapter 包装 provider）已记录为后续候选；`imageRouting` 死分支本次删除而非保留。验收以「桥接开启时文本模型可粘贴、关闭时被拒」的端到端手测为准。
- [卡片重写引入 UI 回归（字段类型、校验提示、冲突提示、折叠分组）] → 保持字段集合与文案不变，只新增「高级」折叠分组并替换数据通路；tasks 中列出逐字段对照验收。
- [attachment API 被误判为已破坏]（`readImage` 仍是 `AttachmentStore` 的 abstract 成员、`dsh-attachment-local` 仍实现，`{ref,data}` 形状不变）→ **不改** `persistence.mjs`；如需 provider 无关性，后续单独 change 迁到 `imageHostPath` / `readImageRequest`。
- [`llm.resolveModelInfo` 猴补丁在新版 HMR/invariant 下的稳定性未验证] → 实现期以「桥接开启时文本模型可粘贴、关闭时被拒」的端到端手测作为验收；`imageRouting` 前向兼容块在新版是死代码，保留但加注说明（或顺手删除，二选一，不影响契约）。
- [锁文件/peer 范围不一致导致发布期失败] → D6 的锁文件要求 + CI 冻结安装作业在 PR 期即暴露。

## Migration Plan

1. 依赖与锁：更新主 `package.json`（peer + dev + `dsh.client`/`exports["./client"]`），重生成 `pnpm-lock.yaml`。
2. host 半：`lib/index.mjs`、`lib/image-bridge/index.mjs` 的 settings 注册迁到 `installSection`（D2）；确认工具运行态仍走 `source()`；桥接侧删除 `ctx.imageRouting` 死分支并给 `resolveModelInfo` 补丁加安装前能力检查（D7）。
3. 设置面：**先做渲染 spike**——在本机 0.1.5 上以真实 `dsh web` 读 boot graph，确认卡片 bundle 被下发（社区有该标签页渲染为空的报告）；随后删除 `lib/settings-route.mjs`、`visionary-settings-card` 行与嵌套子包，把 `dsh.client`/`exports["./client"]` 落到主 `package.json`、浏览器半改写为 `lib/client.js` 的两张卡片（D3 + D9，含「高级」折叠分组）。
4. 测试与 CI：更新既有测试、补新断言、`ci.yml` 增加插件作业（D5）。
5. 本地验证：在 DSH 0.1.5 上 `dsh plugin --profile web add <本地路径>` → `dsh --profile web --dump-config` 出现 `visionary-vision` / `visionary-image-bridge` 行 → 重启后工具目录出现 5 个工具 → Settings → Plugins → Plugin configuration 出现两张 Visionary 卡片、折叠区可展开、写值与 `unset` 重置均热重载 → VL 与文本模型各验收一次粘贴图片路径。
6. 文档：插件 README、`docs/integrations/deepseek-harness.md`、根 README 的配置入口描述改为「Settings → Plugins → Plugin configuration」。
7. 发布：`python3 scripts/bump_version.py 0.7.3 --release`，随后核对 `update-server-json` 与 `dsh-plugin-release` 两个 workflow。

**回滚**：插件为 npm 包，回退到 0.7.2 即可恢复旧 DSH 组合（`dsh plugin add @xlight-oss/visionary-dsh@0.7.2`）；Rust 二进制与工具面契约未变，无需配套回滚。

## Open Questions

- 已决：卡片**不引入** `@deepseek-ai/dsh-client-ui-primitives`，沿用 inline style + `var(--dsw-*)` 主题变量——保持浏览器半除 `react` 外零 DSH 模块边（D4），视觉一致性等有需要时再单独评估。

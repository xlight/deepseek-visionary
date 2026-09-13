# Tasks: 适配 DSH 0.1.5（含配置面迁移到原生设置页）

## 1. 依赖与版本线

- [x] 1.1 `packages/dsh-plugin/package.json`：`peerDependencies` 中 `@deepseek-ai/dsh-{tools,llm,attachment,settings}` 由 `^0.1.0-rc.6` 升到 `^0.1.5-rc.1`（caret 下限，允许 rc.1/rc.2），`devDependencies` 精确锁定 `0.1.5-rc.2`（当前基线，社区约定：peer 下限留在基线之前），`@deepseek-ai/cordis` → `^4.0.2`，`@deepseek-ai/schemastery` → `^3.18.2`
- [x] 1.2 删除 `packages/dsh-plugin/lib/settings-card/package.json`（嵌套子包）：`dsh.client`（`platform: web` + `inject: [@deepseek-ai/dsh-client-locale, @deepseek-ai/dsh-client-ui-settings, @deepseek-ai/dsh-client-ui-settings-plugins]`）与 `exports["./client"]` 改由主 `packages/dsh-plugin/package.json` 声明，浏览器半移至 `lib/client.js`（原因见 3.4 / design D9）；`@deepseek-ai/cordis` peer 同步 `^4.0.2`
- [x] 1.3 在 `packages/dsh-plugin` 重新 `pnpm install` 生成锁文件（不得使用 `--no-lockfile`），确认 `pnpm-lock.yaml` 中 `@deepseek-ai/dsh-*` 解析到 `0.1.5-rc.x` 且 `specifier` 已更新
- [x] 1.4 校验：`npm view "@deepseek-ai/dsh-settings@^0.1.5-rc.1" version --json` 命中 0.1.5-rc.1/rc.2；`node -e "import('@deepseek-ai/dsh-settings').then(m=>console.log(Object.keys(m)))"` 输出含 `SettingsConflictError`（证明新 API 已可用）

## 2. settings 命名空间注册迁移（host 半）

- [x] 2.1 `lib/index.mjs`：删除 `installSettingsSection` / `settingsNamespace` 导入；`SETTINGS_NAMESPACE` 改为字面量 `"visionary-vision"`
- [x] 2.2 `lib/index.mjs`：把注册移入 `ctx.inject(["settings"], (settingsCtx) => settingsCtx.settings.installSection(ctx, SETTINGS_NAMESPACE, Config, config, { setSource, onChange }))`，`owner` 用外层插件 `ctx`
- [x] 2.3 `lib/image-bridge/index.mjs`：同 2.1/2.2（命名空间 `"visionary-image-bridge"`，hooks 额外带 `validate: validateConfig`）；保留 `cleanPasted` 触发器的清理与复位逻辑
- [x] 2.4 确认运行时值仍经 `source()` 读取、`onChange` 在安装时被同步调用一次不会破坏初始 `runtime`（必要时把注册放在 `runtime` 初始化之后）
- [x] 2.5 两处 `ctx.get("settings")?.update(...)` 调用保留（`update(ns, patch, expectedRevision)` 仍在）
- [x] 2.6 桥接（`lib/image-bridge/core.mjs`、`index.mjs`，design D7）：删除 `ctx.imageRouting` 前向兼容分支与「社区咨询契约」注释；`makeModelInfoPatch` 在 `llm.resolveModelInfo.bind` 之前加 `typeof … === "function"` 检查，缺失时记 warn 并按「桥接关闭」继续（不让插件行抛错死掉）

## 3. 配置面迁移到原生设置页

- [x] 3.1 删除 `lib/settings-route.mjs`（含 `mountVisionaryApi`、fenced `/visionary/api` 路由、`SettingsConflictError` 处理）与只被它使用的 `lib/image-bridge/trust-fence.mjs`（连同其单测）
- [x] 3.2 删除 `lib/settings-card/index.mjs`（空操作宿主半已无意义）与 `cordis.patch.yml` 的 `visionary-settings-card` 行；浏览器 bundle 改由主行（`visionary-vision`，裸包名）发现
- [x] 3.3 `lib/index.mjs` 的 `SETTINGS_NAMESPACE` / `lib/image-bridge/index.mjs` 的命名空间仍按原值导出（保持 `settings.yaml` 与既有 patch 配置兼容）
- [x] 3.4 修正浏览器 bundle 的发现契约（design D9）：`dsh-client-modules` 的 `locatePkgJson` 只接受**裸包名**行（`exactPackageSpecifier` 对 `@scope/name/sub` 返回 `undefined` 并在解析前直接丢弃），故 `@xlight-oss/visionary-dsh/settings-card` 行永远拿不到 `dsh.client`。落地：`dsh.client` + `exports["./client"]` 上移主 `package.json`，`lib/client.js` 的注册 id 改为主包名（runner 按包名 require 激活行）

## 4. 客户端卡片重写

- [x] 4.0 渲染 spike（先于 4.1-4.11）：在 DSH 0.1.5-rc.2 上启动真实 `dsh web` 并读取 `window.__DSH_BOOT__`。**结论**：卡槽派发本身正常，缺卡是**本插件**的 bundle 发现缺陷（见 3.4）——修复前 boot graph 里完全没有 `@xlight-oss/visionary-dsh/client.js`，修复后该行出现且 HTTP 200 返回 bundle 本体；故不改落 `settings.plugins.tab`，维持 `settings.plugin.item`
- [x] 4.1 数据通路：用 `ctx.settingsScope.bind({ namespace })` 替换 `callVisionaryApi` + 自建 `createSnapshotStore` 世代管理；`subscribe()` 驱动重绘、`getSnapshot()` 供渲染
- [x] 4.2 写入：保存时以读取到的 `revision` 为栅栏提交 `mutate([{op:'set',path:[field],value}…])`；字段清空用 `{op:'unset'}`；冲突/失败按快照 `status`/错误呈现，不覆盖他人写入
- [x] 4.3 注册两张卡：`ctx.slots.register({ name: "settings.plugin.item", key: "visionary-vision", … }, VisionCard)` 与 `key: "visionary-image-bridge"` 的 `BridgeCard`（`ctx.slots.inject("settings.plugin.item", …)` 包裹）
- [x] 4.4 字段集合与文案保持一致，并新增「高级」折叠分组（默认收起、展开状态为组件本地状态）：Vision 卡主区 = `binaryPath` / `modelType` / `visionTimeoutMs`，折叠区 = `statusTimeoutMs` / `loginTimeoutSeconds`；Bridge 卡主区 = `enabled` / `scope` / `mode`，折叠区 = `promptTemplate` / `pastedDir` / `retainHours` + `cleanPasted` 触发器按钮
- [x] 4.5 `binaryPath` 共享语义保留：Vision 卡在保存该字段时同时写 `visionary-vision.binaryPath` 与 `visionary-image-bridge.binaryPath`（两个 scope，同一修订栅栏流程）
- [x] 4.6 `cleanPasted` 触发器：写 `true` 即触发清理，卡片随后按复位后的值刷新（桥接 host 侧不动）
- [x] 4.7 不可用态：快照 `status` 为 `loading`/`unavailable` 或 `writable === false` 时显示对应状态（加载中 / 不可用 / 只读），不报错崩溃
- [x] 4.8 模块依赖：**删除** `require("@deepseek-ai/dsh-client-runtime/client")`（其唯一用途 `createSnapshotStore` 随快照世代层消失），重写后卡片除 `react` 外不 require 任何 DSH 包
- [x] 4.9 决定是否引入 `@deepseek-ai/dsh-client-ui-primitives`（design Open Question）：不引入则沿用 inline style + `var(--dsw-*)` 主题变量
- [x] 4.10 确认 bundle 契约：`window.__ModuleLoader__.load({ id: "@xlight-oss/visionary-dsh", factory })`（id 必须等于包名，runner 以包名 require 激活该行）、`exports["./client"]` → `lib/client.js`
- [x] 4.11 覆盖态呈现：快照 `user` 层非空的字段标注「已覆盖（重置回到默认）」，重置按钮走 `settingsScope.unset(field)`；无差异则不显示该提示

## 5. 测试与 CI

- [x] 5.1 `test/tools.test.mjs`：伪 settings 服务改为 `installSection` 契约（`setSource`/`onChange`），断言 modelType 热重载仍生效
- [x] 5.2 `test/integration-smoke.test.mjs`：改为注入实现 `installSection` 的伪 settings 服务；新增断言「无 settings 服务时插件不报错且工具按 entry 配置工作」
- [x] 5.3 新增卡片/槽位单测：`settings.plugin.item` 注册两个 key；写入走 `settingsScope` 的 `set`/`mutate` 且带 revision；不可用态降级
- [x] 5.4 `.github/workflows/ci.yml` 增加插件作业：`pnpm install --frozen-lockfile` + `node --test`（`working-directory: packages/dsh-plugin`）
- [x] 5.5 全绿校验：`cd packages/dsh-plugin && pnpm install && node --test`
- [x] 5.6 新增发现契约回归测试（`integration-smoke.test.mjs` 的 `client discovery`）：bundle patch 至少有一个裸包名行、每个子路径行都不可承载 `dsh.client`、`exports["./client"]` 指向存在的文件、bundle 注册 id 等于包名（守住 3.4）

## 6. 文档

- [x] 6.1 `packages/dsh-plugin/README.md`：配置入口由「设置面板 → 左侧导航 Visionary」改为「Settings → Plugins → Plugin configuration」，删除私有路由/白名单说明，补 DSH ≥ 0.1.5-rc.1 的新最低版本要求
- [x] 6.2 `docs/integrations/deepseek-harness.md`：同步配置入口与版本要求
- [x] 6.3 根 `README.md` 的 DSH 段落（第 4 节与插件说明）同步
- [x] 6.4 `scripts/bump_version.py`：删除对已不存在的 `lib/settings-card/package.json` 的版本同步（一致性校验回到 7 个条目）

## 7. 端到端验证与发布

- [x] 7.1 `dsh plugin --profile web add <本地路径>` 安装本地包，`dsh --profile web --dump-config` 出现 `visionary-vision` / `visionary-image-bridge` 两个行，无加载错误；浏览器侧 boot graph 含 `@xlight-oss/visionary-dsh/client.js` 且该 URL 返回 200。发布形态另用 `npm pack` 产物复核：tarball 含 `lib/client.js`(31604B) / `cordis.patch.yml`（恰好两行）且 manifest 的 `dsh.client` 与 `exports["./client"]` 正确，无遗留 `./settings-card` 导出
- [x] 7.2 重启 DSH 后工具目录出现 5 个工具（`deepseek_vision` / `deepseek_ocr` / `status` / `login` / `logout`）——真实 web profile 重启宿主后确认（宿主 14:24:29 重启，本会话工具目录含全部 5 个）
- [x] 7.3 设置页验收（真实浏览器 + 真实 `dsh web`，CDP 驱动 DOM）：Settings → Plugins → 插件配置 出现「Visionary · 视觉工具」与「Visionary · 图片桥接」两张卡；卡片读到的是组合 entry 生效值（300000/60000/600）；「高级」折叠区可展开且不持久化；改 `visionary-vision.visionTimeoutMs` → 保存 → 宿主 `settings.yaml` 落盘 `240000` 且页面回显；用户层被覆盖的字段标注「已覆盖」并出现「重置」，重置后标记消失、`settings.yaml` 的该字段被移除（`unset`）
- [x] 7.4 桥接验收：**文本模型路径已在真实宿主实测**——本会话在真实 `dsh web` 里读取一张本地截图时，工具结果被桥接改写为「已保存到 `C:\Users\xLight\.deepseek-visionary\pasted\sha256_….png`。请使用 deepseek_vision 工具分析该图片」（正是本插件 `promptTemplate` 的落盘 + 引导文本），随后 `deepseek_vision_status` 经插件正常返回；VL 路由不干预与 `enabled: false` 恢复拒绝由单测/集成覆盖（`core.test.mjs` 按路由匹配仅补报 `image`、`integration-smoke.test.mjs` 的补丁开关与卸载还原）。未做的两项：真实 VL 模型会话手测、真实 `deepseek_vision` 分析成功——后者当时被 vision 后端故障（`fork response missing id`）挡住，与插件无关
- [x] 7.5 二进制解析回归：由 `test/binary-resolution.test.mjs` 覆盖并通过（含 `lazy resolution: env change takes effect without re-apply`——设置 `DEEPSEEK_VISIONARY_BIN` 后无需重新 apply 即生效；`Config.binaryPath` → env → PATH 优先级与 npm shim 解析在同文件断言），Windows 本机实测通过；未再做人工端到端（需真实会话）
- [x] 7.6 浏览器无控制台模块解析错误：CDP `Runtime.exceptionThrown` + `Log.entryAdded` 监听整页 reload（`ignoreCache: true`）→ **0 条 error/exception**，且两张卡片正常物化（若缺 `react` 边会先抛 `missed the module table`、卡片不会出现）
- [ ] 7.7 `python3 scripts/bump_version.py 0.7.3 --release` 发布；核对 `dsh-plugin-release`、cargo-dist、`update-server-json` 三个 workflow 成功，`server.json` 的 `fileSha256` 回填为 v0.7.3 真实哈希

## 8. 卡片外观对齐宿主 `PluginCard`（用户实测反馈后补做，design D10）

- [x] 8.1 取证：从 `dsh-client-ui-settings-plugins` 客户端 bundle 取出卡片 chrome 与字段区的全部 CSS 规则与结构（`li.card` / `header` / `cardOpen` / `body` / `field`+分隔线 / `label` / `input`（34px、`:focus-visible` 用 brand-primary）/ `hint` / `footer` / `discard`+`save`（label-primary 底）/ `Tag(neutral)`「已覆盖」/ 文本按钮「恢复默认」/ 保存成功后收起）
- [x] 8.2 `lib/client.js`：卡片重写为 `<li class="vlb-card">` 折叠行 + `aria-expanded` 头部（15px/600 标题、13px 描述、dirty 时「未保存」标记、14px 内联 chevron），展开体 `border-top:.5px; margin:0 16px; padding-bottom:8px`，字段含 `.5px` 分隔线与 `label[for]`→控件 `id`，数字框 `inputmode="numeric"` + `aria-invalid`，保存/放弃按宿主配色，保存成功后自动收起
- [x] 8.3 样式承载：`CARD_CSS` 由 `apply()` 以 `ctx.effect` 注入一张 `<style data-plugin="@xlight-oss/visionary-dsh">`，fiber 卸载即移除（无 document 的环境跳过，保证单测可跑）
- [x] 8.4 布尔字段改用宿主 `Switch`（`@deepseek-ai/dsh-client-ui-primitives` 平台 seed，`require` 外包 try/catch，缺席时退回 checkbox），并在英文/中文字典补 `expand`/`collapse`，`reset` 文案改为「恢复默认 / Restore default」
- [x] 8.5 单测：改为按 `vlb-*` 类名断言，新增 7 个用例（样式注入与卸载、折叠行与 `aria-*`、dirty「未保存」+保存后收起、Switch 而非 checkbox、label/aria-invalid、模块边含 primitives）
- [x] 8.6 真实浏览器验证（无头 Chrome + CDP）：`getComputedStyle` 与我们/宿主原生卡片的 `li`/header/name/description/body/field/label/input/hint/save/discard/footer/chevron 逐项比对**一致**；布尔字段为 `button[role="switch"]`；点开关 → 「未保存」→ 保存 → 宿主 `settings.yaml` 写入 `visionary-image-bridge.enabled: false` → 卡片收起

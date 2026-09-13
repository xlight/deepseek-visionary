# adapt-dsh-0-1-5

## Why

本插件依赖的两个 DSH 契约**早已被上游移除**，而不是到 0.1.5 才发生的：`@deepseek-ai/dsh-settings` 的 `installSettingsSection` / `settingsNamespace` 实测在 0.1.0-rc.8 仍导出、**自 0.1.2-alpha.2 起已消失**；浏览器模块表里的 `@deepseek-ai/dsh-client-runtime` 同期被删除（seed 行改为 `@deepseek-ai/dsh-client-store`）。当前 DSH 线为 0.1.5（npm `latest` = 0.1.5-rc.1、`next` = 0.1.5-rc.2）。

结果是插件**自 0.1.1 / 0.1.2 线起完全不可用**：两个 host 插件行在模块加载期即失败（命名导出缺失，整个插件行死掉），5 个原生工具、图片桥接、设置面板全部缺失；客户端半的 `require("@deepseek-ai/dsh-client-runtime/client")` 无法在模块表中解析。

`peerDependencies: ^0.1.0-rc.6` 并不能兜底：该范围受 semver 预发布规则限制只解析到 0.1.0-rc.8（仍是旧 API），且 DSH profile 的 `pnpm-workspace.yaml` 为 `autoInstallPeers: false`、profile 内不存在 `@deepseek-ai/*`——宿主按「in-box bundle 只从 dsh 安装解析」的契约提供这些包。**即插件既拿不到新 API，也没有可自带的旧副本保命，迁移是强制的**；peer 范围的作用是契约声明，而非可用性来源。

而 DSH 已把「仓库外插件配置」做成正式设计（上游 Agent Note `2026-08-12-plugin-owned-settings-surface`：**"Registering is exposing"**——api-proxy 服务 `settings.describe()` 返回的**每个**命名空间且不再有 `WEB_SETTINGS_NAMESPACES` 白名单；`settings.plugin.item` 改为**按命名空间 keyed**，并明确 *"A plugin distributed outside this repository is configurable from the settings page with no change here"*）：客户端提供 `ctx.settingsScope.bind()`，内建 Settings → Plugins 的插件配置页按命名空间配对卡片。社区同题插件 `@goodandready/dsh-vision-bridge` 已按 `ctx.settingsScope` 接入原生设置面，可作对照。插件自写的私有 `/visionary/api` 路由 + 607 行手写客户端因此从「必要的绕过」变成「多余的平行机制」，正是收拢这笔债的时机。

> 参考：[上游 Agent Note](https://github.com/Truly-Private/oh-my-deepseek-harness/blob/d7d622923f5f943de4aadf8600f116f13fc03eef/.agents/notes/implemented/architecture/2026-08-12-plugin-owned-settings-surface.md)（原路径 `deepseek-harness/.agents/notes/implemented/architecture/2026-08-12-plugin-owned-settings-surface.md`）、[社区插件构建指南 · Client Slot 与主题](https://github.com/oil-oil/build-deepseek-harness-plugin/blob/main/references/client-slots-and-theme.md)、[dsh-vision-bridge](https://github.com/GooDAnDReaDY/dsh-vision-bridge)。

## What Changes

- **BREAKING**：设置命名空间注册迁移到当前 DSH 契约 `ctx.settings.installSection(owner, ns, schema, entry, hooks)`（在 `ctx.inject(["settings"], …)` 内挂载；命名空间改为字面量字符串），插件最低支持版本提升到 DSH 0.1.5 线，**放弃 0.1.2-alpha.2 之前（含 0.1.0-rc.x）**。选 `installSection` 而非同服务的 `register(ns, schema)`：本项目设置**就是** cordis 行配置（`entry` = 行配置对象），需要写回行配置；社区插件用 `register` 是因为它们读的是独立文档而非行配置
- **BREAKING**：依赖线从 `^0.1.0-rc.6` 升到 `^0.1.5-rc.1`（peer caret 下限，覆盖 rc.1 / rc.2），`devDependencies` 精确锁定当前基线 `0.1.5-rc.2`（社区做法），`@deepseek-ai/cordis` → `^4.0.2`、`@deepseek-ai/schemastery` → `^3.18.2`，并重生成 `pnpm-lock.yaml`（peer 为契约声明；DSH API 由宿主提供，见 Why）
- 韧性：设置 API 缺失/版本不符时不再以模块加载期异常让整行插件死掉（改用命名空间导入 + 运行时检查，给出明确的不兼容提示或降级）；具体取舍在 design 定
- 修复客户端半模块依赖：`require("@deepseek-ai/dsh-client-runtime/client")` 只用于手写快照世代层（`createSnapshotStore`，`client.js:8,184`），随该层一并消失，即**不再跨包 require**；若重写后仍需要 store，则改用当前 shell 种子词 `@deepseek-ai/dsh-client-store`。`dsh.client.inject` 仅作加载元数据声明（不参与 apply 顺序），改为列 `@deepseek-ai/dsh-client-locale` 与提供 `settingsScope` 的 `@deepseek-ai/dsh-client-ui-settings`（非基线模块的实际请求由同处的 `dsh.client.external` 承载）
- **删除**插件私有设置路由（`lib/settings-route.mjs` 与 `/visionary/api` 挂载）：原生 settings Remote 已服务所有命名空间，写冲突由修订栅栏（`expectedRevision`）承担，无需自建 403/409 语义
- 配置面迁入**原生插件设置区**（落点已定稿）：两个命名空间各注册一张 `settings.plugin.item` 卡片（options `key` = 命名空间），出现在 Settings → Plugins → Plugin configuration；卡片分「主区 + 折叠的高级区」，桥接的一次性触发器收进折叠区；传输层为 `ctx.settingsScope.bind({ namespace })`（`getSnapshot` / `subscribe` / `set` / `unset` / `mutate`，带修订栅栏），不再自行 fetch 与维护快照世代
  - 社区对照（本地源码 + npm 抽样）：`settings.section` 自有整页仍是社区多数（16 个明确注册中 9 个），本机两个活跃插件 `dsh-better-sidebar@0.19.1`、`dsh-pocket@2.10.6` 也都走自有整页 + 私有 host 通道；但 `settings.plugin.item` 有真实先例（`@liustack/modlens`、`dsh-plugin-model-proxy`）且官方 cookbook 明确面向仓库外插件，而本项目两个命名空间都是小表单，故按原生落点走并在实现期先做渲染 spike（该槽位有社区字段缺陷报告，见 design 风险条）；`settings.plugins.tab` 与 `settings.section` 两个备选一并弃用（理由见 design D3）
- 保留不变：5 个原生工具面与参数 schema、图片桥接的对外语义（admission 放行 + `llm/stream` 改写——admission gate 仍硬编码拒绝文本模型图片、原生占位符不含路径，且无插件扩展点；本插件继续走对用户透明的 `resolveModelInfo` 补丁，社区另有「注册合成 provider 让用户改选模型」的 adapter 路线，见 design D7）、二进制解析与懒解析、版本不匹配警示、`COMPAT_MINOR = 0.7`（Rust 二进制契约不变）；顺带删掉 `ctx.imageRouting` 前向兼容死分支（该契约无任何已发布宿主实现与消费），并给补丁加安装前的能力检查
- CI 增加插件测试作业：当前 `ci.yml` 只跑 cargo，`packages/dsh-plugin` 的 `node --test` 从未在 CI 执行，本次 API 漂移正是因此未被拦截
- 文档与版本：更新插件 README、`docs/integrations/deepseek-harness.md`、根 README 的 DSH 接入描述；`scripts/bump_version.py` 移除对已删除的嵌套子包 `lib/settings-card/package.json` 的版本同步（一致性校验回到 7 个条目）；随 0.7.3 一同发布

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `dsh-plugin`: 设置命名空间注册改用当前 DSH 的 `ctx.settings.installSection` 契约；配置面由插件自写页面 + 私有路由迁入原生插件设置区（`settings.plugin.item` 按命名空间两张卡，传输层 = `settingsScope`）；客户端半不再跨包 require（去掉 `dsh-client-runtime` 边，并按社区写法声明 `dsh-client-ui-settings`）；显式声明受支持的 DSH 版本线，peer 依赖与实际契约一致

## Impact

- **代码**：`packages/dsh-plugin/lib/index.mjs`、`lib/image-bridge/{index.mjs,core.mjs}`（后者删除 `imageRouting` 死分支并加补丁能力检查）、浏览器半由 `lib/settings-card/client.js` 重写并移至 `lib/client.js`（主 manifest 的 `dsh.client` + `exports["./client"]` 承载发现）、**删除** `lib/settings-card/{index.mjs,package.json}`（嵌套子包）、`lib/settings-route.mjs` 与 `lib/image-bridge/trust-fence.mjs`（连同其单测）、`cordis.patch.yml` 的 `visionary-settings-card` 行、`packages/dsh-plugin/package.json`、`pnpm-lock.yaml`、`.github/workflows/ci.yml`、`scripts/bump_version.py`
- **测试**：`test/tools.test.mjs`、`test/integration-smoke.test.mjs` 中的伪 settings 服务改按新 API 构造；新增「命名空间注册在 settings 服务缺失时回退 entry 配置」与「卡片按命名空间注册」相关断言
- **依赖**：`@deepseek-ai/dsh-{tools,llm,attachment,settings}` `^0.1.5-rc.1`、`@deepseek-ai/cordis` `^4.0.2`、`@deepseek-ai/schemastery` `^3.18.2`。客户端半已核实：`@deepseek-ai/dsh-client-runtime` 在 0.1.5 树内整体不存在，唯一用途随快照世代层移除；若仍需 store，`@deepseek-ai/dsh-client-store` 已在 shell 种子词表内（属基线，无需声明即可解析）；`settingsScope` 所在的 `@deepseek-ai/dsh-client-ui-settings` 不属基线，列入 `dsh.client.inject` 作为提供方标注（该字段只是加载元数据，社区里声明与否两种做法都有，真正的等待靠 cordis 服务 inject）
- **兼容**：不做回兼容——自 0.1.2-alpha.2 起本插件已不可用（见 Why），本次只声明 0.1.5 线（peer `^0.1.5-rc.1`，覆盖 rc.1 / rc.2）；0.1.2-alpha.2 之前的旧 API 明确不再支持。工具面、CLI 契约、Rust 二进制与 `~/.deepseek-visionary/` 凭据格式不变
- **文档**：`packages/dsh-plugin/README.md`（配置面板章节改写）、`docs/integrations/deepseek-harness.md`、`README.md`（DSH 接入段落）——配置入口说明改为「Settings → Plugins → Plugin configuration 下的两张 Visionary 卡片」，并补一小节「支持的 DSH 版本线」，参照社区做法（peer 下限 + devDependency 精确锁定、按 DSH 版本给升级矩阵）
- **发布**：单版本策略下仍以一次 `bump_version.py 0.7.3 --release` 同时发布 Rust 二进制、Zed 扩展与插件；`dsh-plugin-release.yml` 用 `--frozen-lockfile`，锁文件必须随提交更新

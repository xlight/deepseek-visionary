# dsh-plugin (DSH 插件)

## MODIFIED Requirements

### Requirement: 插件包结构与安装

插件包 SHALL 是一个标准 DSH bundle，满足以下结构要求：

- `package.json` 声明 `dsh.bundle.patch` 指向自带 `cordis.patch.yml`，使 `dsh plugin --profile <name> add <pkg>` 安装后自动追加到 profile 的 `dsh.profiles.bundles` 层叠，用户无需手写任何配置
- `cordis.patch.yml` SHALL 以 `- insert:` 形式注册插件行（`id` + `name` 指向包名）
- `lib/index.mjs` SHALL 导出 Cordis 插件契约：`name`（与 patch 行 `id` 一致）、`inject`（含 `tools`）、`Config`（schemastery schema）、`apply(ctx, config)`
- 包 SHALL 为纯 ESM 普通 JavaScript，无构建步骤
- `peerDependencies` SHALL 声明 `@deepseek-ai/cordis`、`@deepseek-ai/dsh-tools`、`@deepseek-ai/dsh-settings`、`@deepseek-ai/dsh-llm`、`@deepseek-ai/dsh-attachment`、`@deepseek-ai/schemastery`，且版本范围 SHALL 覆盖插件实际依赖的 DSH 契约（peer caret 下限 `^0.1.5-rc.1`，同一 0.1.5 线）；`devDependencies` SHALL 精确锁定当前基线（`0.1.5-rc.2`，不加范围符），`pnpm-lock.yaml` SHALL 由该范围生成并随提交更新
- 包名 SHALL 为 `@xlight-oss/visionary-dsh`（与既有 npm 通道 `@xlight-oss/visionary-server` 同 scope）
- 客户端半（`lib/client.js`）SHALL 只 `require` 平台种子词提供的模块（`react` 与 `@deepseek-ai/dsh-client-ui-primitives`）；SHALL NOT 依赖已被上游移除的模块 id（如 `@deepseek-ai/dsh-client-runtime`），SHALL NOT 引入插件自有模块边；`settingsScope` 等服务依赖 SHALL 经 cordis `inject` 等待，其提供方 SHALL 在 `dsh.client.inject` 中作为加载元数据声明
- 卡片外观 SHALL 与宿主自带的插件卡（`PluginCard`）一致：`<li>` 折叠行 + `aria-expanded` 头部（15px/600 标题、13px 描述、14px chevron、dirty 时「未保存」标记），展开后为 `.5px` 分隔的字段区（label 13px/500、输入控件 34px 高、`:focus-visible` 用 `--dsw-alias-brand-primary` 描边），`保存` 用 `--dsw-alias-label-primary` 底 + `--dsw-alias-bg-layer-3` 字、`恢复默认` 为文本按钮；样式 SHALL 由插件自注入的一张 `<style data-plugin>` 承载并随 fiber 卸载；布尔字段 SHALL 使用宿主 `Switch` 而非浏览器默认 checkbox；保存成功后卡片 SHALL 自动收起
- 浏览器 bundle SHALL 通过**主包 manifest** 暴露：`package.json` 声明 `dsh.client`（`platform: "web"`）与 `exports["./client"]` → `./lib/client.js`，`cordis.patch.yml` SHALL 至少保留一个**裸包名**（`@xlight-oss/visionary-dsh`）行；SHALL NOT 依赖子路径行名承载浏览器半（`dsh-client-modules` 的 `exactPackageSpecifier` 会丢弃 `@scope/name/sub` 形式，静默不下发 bundle）。bundle 内 `window.__ModuleLoader__.load({ id })` 的 `id` SHALL 等于包名（runner 以包名 require 激活该行）

安装方式 SHALL 支持：`dsh plugin --profile web add @xlight-oss/visionary-dsh`（npm）与本地路径安装（`dsh plugin --profile web add <path>`）。

#### Scenario: 一键安装插件

- **WHEN** 用户执行 `dsh plugin --profile web add @xlight-oss/visionary-dsh` 并重启 DSH（版本 ≥ 0.1.5-rc.1）
- **THEN** 插件行出现在组合配置中，`ctx.tools` 注册 `deepseek_vision` 等工具，模型可调用

#### Scenario: bundle 自注册

- **WHEN** 检查安装后的 profile 组合配置（`dsh --profile web --dump-config`）
- **THEN** 出现 `@xlight-oss/visionary-dsh` 的 bundle 层，含 `visionary-vision` 插件行，无需用户编辑 cordis.patch.yml

#### Scenario: 客户端半只依赖当前 shell 提供的模块

- **WHEN** 插件行加载且浏览器物化设置卡片 bundle
- **THEN** bundle 的每个 `require` 都命中平台种子词（`react`、`@deepseek-ai/dsh-client-ui-primitives` 等），无「missed the module table」失败

#### Scenario: 卡片与宿主卡片外观一致

- **WHEN** 用户在 Settings → Plugins → Plugin configuration 查看本插件的卡片
- **THEN** 卡片呈现为与宿主自带卡片相同的折叠行（同名同级标题/描述、右侧 chevron，dirty 时「未保存」标记），展开后字段区、输入框与保存/放弃按钮的边框、圆角、字号与配色 SHALL 与宿主卡片一致（按 `getComputedStyle` 逐项比对），布尔字段呈现为宿主的 Switch 控件

#### Scenario: 浏览器 bundle 被宿主发现并下发

- **WHEN** 宿主以 web profile 启动后读取页面 boot graph（`window.__DSH_BOOT__`）
- **THEN** 存在 `@xlight-oss/visionary-dsh` 行且其 `url`（`/plugins/??@xlight-oss/visionary-dsh/client.js&rev=…`）返回 200 与 bundle 本体；页面加载该 bundle 后以其**包名**注册工厂（无 `no registered factory` 失败）

#### Scenario: 依赖范围覆盖宿主实际契约

- **WHEN** 解析 `peerDependencies` 中的 `@deepseek-ai/dsh-settings` 等条目
- **THEN** 解析结果落在宿主实际运行的 0.1.5 线（如 `0.1.5-rc.2`），而非 0.1.0-rc.x

### Requirement: 设置面板上传路径配置

插件 SHALL 在 DSH 原生设置面（Settings → Plugins → Plugin configuration）提供 `modelType` 配置项（`vision` 默认 | `ocr`），覆盖 CLI 默认值，修改后 SHALL 即时生效（热重载），作用于 `deepseek_vision` 工具的上传管道。该配置 SHALL 呈现为该插件在 `visionary-vision` 命名空间下的原生插件卡片（注册于 `settings.plugin.item`，`key` = 命名空间），卡片 SHALL 通过客户端 settings scope（`ctx.settingsScope.bind({ namespace })`）读取快照并写入，写入 SHALL 携带读取时的命名空间修订号作为栅栏。低频字段（`statusTimeoutMs`、`loginTimeoutSeconds`）SHALL 置于默认收起的「高级」区，且该折叠状态 SHALL NOT 写入设置文档；字段值被用户层覆盖时卡片 SHALL 可标示并支持重置回默认（`unset`）。

#### Scenario: 设置面板切换上传管道

- **WHEN** 用户在 Settings → Plugins → Plugin configuration 的 Visionary 卡片将 modelType 改为 ocr 后调用 `deepseek_vision`
- **THEN** `deepseek_vision` 走 OCR 管道（spawn `visionary-server vision <image> --json --model-type=ocr`），无需重启 DSH

#### Scenario: 写入按修订栅栏拒绝陈旧编辑

- **WHEN** 卡片的草稿基于修订号 N，而该命名空间已被另一写入推进到 N+1，用户随后保存
- **THEN** 保存被拒绝并提示冲突（不覆盖新值），卡片重新读取当前值

### Requirement: 桥接配置面板扩展

插件 SHALL 在 DSH 原生设置面提供桥接配置项：`scope`（`text-only` 默认 | `also-vl`）、`mode`（`agentic` 默认 | `deterministic`），并提供触发"清理已落盘副本"的操作（`cleanPasted` 触发器字段，切换后清理并自动复位，见 design D3）。这些项 SHALL 呈现为该插件在 `visionary-image-bridge` 命名空间下的原生插件卡片（注册于 `settings.plugin.item`，`key` = 命名空间）；修改后 SHALL 即时生效（热重载）。`promptTemplate`、`pastedDir`、`retainHours` 与 `cleanPasted` 触发器 SHALL 置于默认收起的「高级」区。

#### Scenario: 桥接范围切换

- **WHEN** 用户在原生插件配置页将桥接 scope 改为 also-vl
- **THEN** 桥接开始对 VL 模型同样改写图片（此前 VL 模型原生看图不干预）

#### Scenario: 桥接模式切换

- **WHEN** 用户在原生插件配置页将桥接 mode 改为 deterministic
- **THEN** 桥接自行调用分析并将结果文本注入（不再仅依赖模型自主调用工具）

#### Scenario: 手动清理落盘副本

- **WHEN** 用户触发卡片上的"清理已落盘副本"（切换 `cleanPasted` 触发器）
- **THEN** 桥接清理 `pastedDir` 下副本（按配置保留策略或全部），并报告清理数量；附件库对象不受影响

## ADDED Requirements

### Requirement: 设置命名空间注册遵循 DSH 0.1.5 契约

插件 SHALL 通过 `ctx.settings.installSection(owner, ns, schema, entry, hooks)` 注册自有命名空间（`visionary-vision`、`visionary-image-bridge`），且 SHALL 在 `ctx.inject(["settings"], …)` 的就绪回调内完成注册，使 settings 服务缺失时插件以组合 entry 配置继续工作。命名空间 SHALL 为字面量小写连字符标识符。`hooks` SHALL 只使用 `setSource(thunk)`、`onChange()` 与可选 `validate`。插件 SHALL NOT 依赖已移除的 `installSettingsSection` / `settingsNamespace` 导出。

#### Scenario: settings 服务在位时命名空间可被配置面板读写

- **WHEN** 宿主组合包含 settings provider，插件行加载
- **THEN** `visionary-vision` 与 `visionary-image-bridge` 出现在 `settings.describe()` 的命名空间列表中，插件运行态值随提交热重载

#### Scenario: settings 服务缺失时以 entry 配置工作

- **WHEN** 宿主未挂载 settings provider（headless 组合）
- **THEN** 插件不注册命名空间但不报错，工具与桥接按组合 entry 配置运行

#### Scenario: 写入非法配置被拒绝

- **WHEN** 通过 settings 通道写入不含 `{path}` 占位符的 `promptTemplate`
- **THEN** 注册时提供的 `validate` 拒绝该写入，已生效的配置保持不变

### Requirement: 配置面走原生设置通道（无私有路由）

插件的浏览器半 SHALL 只通过 DSH 原生设置通道读写配置（settings Remote + 客户端 settings scope）；插件 SHALL NOT 自建 HTTP 路由、私有 RPC 或自定义快照同步层来服务配置面板。宿主未向该客户端暴露某命名空间时，卡片 SHALL 呈现不可用状态而不是报错崩溃。

#### Scenario: 不存在插件私有设置路由

- **WHEN** 检查已加载插件注册的 webServer 路由
- **THEN** 没有插件自有前缀路由（如 `/visionary/api`），配置读写全部经原生 settings Remote

#### Scenario: 命名空间未暴露时卡片降级

- **WHEN** 客户端连到未暴露该命名空间的宿主（或 settings 处于 memory 模式）
- **THEN** 卡片显示不可用/只读状态，插件其余功能不受影响

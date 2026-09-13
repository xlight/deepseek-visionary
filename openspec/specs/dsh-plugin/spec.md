# dsh-plugin Specification

## Purpose
定义 DeepSeek Visionary 的 DeepSeek Harness 原生插件包（npm 分发，纯 ESM 无构建）：经 `dsh.bundle.patch` 自注册的 Cordis bundle，向 `ctx.tools` 注册 `deepseek_vision` 等原生工具，工具通过 spawn `visionary-server` CLI（vision/status 经 `--json` 原子输出；login/logout 按子命令参数面透出文本）复用 Rust 视觉管道，提供结构化参数 schema 与宿主级权限（不经 bash 沙箱）。
## Requirements
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

### Requirement: 原生工具面
插件 SHALL 经 `ctx.tools.register(defineTool(...))` 注册 5 个原生工具，命名与 MCP 工具一致（`deepseek_vision` / `deepseek_ocr` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`），参数 schema 与对应 MCP 工具对齐：

- `deepseek_vision`：`image`（必填，本地路径 / base64 / data URI）、`prompt`、`thinking`、`continue_conversation`、`session_id`
- `deepseek_ocr`：`image`（必填，本地路径 / base64 / data URI）、`prompt`、`thinking`、`continue_conversation`、`session_id`（OCR 文本提取，等价 `visionary-server ocr <image>`）
- `deepseek_vision_status`：无参数，输出登录状态
- `deepseek_vision_login`：浏览器自动登录（阻塞等待，超时可配置）
- `deepseek_vision_logout`：清除保存的凭据

工具 SHALL 按各子命令的真实参数面 spawn `visionary-server`：
- `deepseek_vision` SHALL spawn `visionary-server vision <image> --json` 并解析原子 JSON，返回 `{"text", "session_id", "parent_message_id"}` 的文本投影并在结果中携带会话信息
- `deepseek_ocr` SHALL spawn `visionary-server ocr <image> --json` 并解析原子 JSON，返回相同形状
- `deepseek_vision_status` SHALL spawn `visionary-server status --json` 并解析原子 JSON
- `deepseek_vision_login` / `deepseek_vision_logout` SHALL spawn `visionary-server login` / `visionary-server logout`（**不带** `--json`——这两个子命令无该参数），直接透出文本输出；`login` 的阻塞时长由 `timeoutMs` 兜底

选项传参 SHALL 一律使用等号形式（`--prompt=<value>` / `--session-id=<value>`），不用空格分隔——CLI（clap）将 `-` 开头的空格形式值当作 flag 拒绝。

#### Scenario: 识图
- **WHEN** 模型调用 `deepseek_vision` 传入图片路径与问题
- **THEN** 插件 spawn `visionary-server vision <image> --json --prompt=<q>`（等号传参），返回视觉模型回答文本与 session_id，工具调用成功

#### Scenario: OCR 提取文字
- **WHEN** 模型调用 `deepseek_ocr` 传入图片路径
- **THEN** 插件 spawn `visionary-server ocr <image> --json`（等号传参），返回提取的文字文本与 session_id，工具调用成功

#### Scenario: 状态检查
- **WHEN** 模型调用 `deepseek_vision_status`
- **THEN** 插件 spawn `visionary-server status --json`，返回 authenticated / token_valid 等字段；未登录时结果说明登录指引

#### Scenario: login 经工具调用
- **WHEN** 模型调用 `deepseek_vision_login`
- **THEN** 插件 spawn `visionary-server login`（不带 `--json`），透出登录结果文本；超时由 `timeoutMs` 终止子进程并返回超时错误

### Requirement: 图片输入处理
`deepseek_vision` 的 `image` 入参 SHALL 区分两种形态处理：
- 本地路径：直接作为 `vision` 的位置参数传递
- base64 / data URI：SHALL 先解码写入临时文件（`os.tmpdir()`），将临时文件路径作为位置参数传递，调用完成后删除临时文件——避免 base64 作为 argv 超出平台单参数大小限制（Linux 单参数上限 131072 字节，真实截图 base64 远超，spawn 会 E2BIG）

#### Scenario: 大图 base64 输入
- **WHEN** 模型传入一张数 MB 截图的 base64 作为 `image`
- **THEN** 插件解码写入临时文件并传路径完成分析，临时文件在调用结束后被清理，无 E2BIG 失败

#### Scenario: 本地路径直传
- **WHEN** 模型传入本地图片路径作为 `image`
- **THEN** 插件直接以该路径调用 `vision`，不复制文件

### Requirement: 工具超时与中止传播
每个工具 SHALL 在 `ToolDefinition` 上声明 `timeoutMs`（DSH 仅对声明超时的工具施加 deadline，且声明即承诺与 `exec.signal` 协作）。所有工具 SHALL 将 `exec.signal` 转发至其 spawn 的子进程（信号中止时 SHALL kill 子进程并返回中止/超时结果），使 DSH 的模型取消与超时能实际终止 CLI 调用。

超时声明 SHALL 为：`deepseek_vision_login` 声明 `timeoutMs = Config.loginTimeoutSeconds × 1000`；`Config.loginTimeoutSeconds` 默认值 SHALL 读取 `DEEPSEEK_LOGIN_TIMEOUT` 环境变量（与 CLI 自身登录超时一致），未设置时默认 600 秒；`deepseek_vision` 声明默认 300000ms（可配置，`Config.visionTimeoutMs`）；`deepseek_vision_status` / `deepseek_vision_logout` 声明默认 60000ms。

#### Scenario: login 超时有界
- **WHEN** 模型调用 `deepseek_vision_login` 且用户在超时内未完成登录
- **THEN** 子进程被终止，工具返回超时错误（提示可重试 `deepseek_vision_login`），DSH 会话不受影响

#### Scenario: 模型取消中止子进程
- **WHEN** 模型在 `deepseek_vision` 执行中取消该工具调用
- **THEN** 插件收到 `exec.signal` 中止信号并 kill 子进程，无残留进程

### Requirement: 二进制解析与错误处理
插件 SHALL 按以下优先级解析 `visionary-server` 二进制：`Config.binaryPath`（显式配置）→ `DEEPSEEK_VISIONARY_BIN` 环境变量 → PATH 查找。win32 下 PATH 扫描 `.exe` 失败时，SHALL 继续解析 npm 全局安装生成的 `.cmd` / `.ps1` shim 定位 exe 真身（见「Windows npm 全局安装场景下自动定位二进制真身」）。二进制路径解析 SHALL 在每次工具调用时重新执行，不复用 `apply()` 时的解析结果（见「二进制解析即时生效（懒解析）」）。解析失败或二进制缺失时，工具调用 SHALL 返回清晰错误（含平台对应的安装指引，见「错误提示平台化」），不崩溃。

未登录时 `deepseek_vision` SHALL 返回登录指引（提示调用 `deepseek_vision_login` 或注入 `DEEPSEEK_USER_TOKEN`）。`vision` 失败（非零退出码）时 SHALL 透出 `{"error"}` 中的信息；`login` 超时（`Config.loginTimeoutSeconds`，默认读取 `DEEPSEEK_LOGIN_TIMEOUT` env、未设置时 600 秒）SHALL 返回超时错误。

工具 SHALL 以 CLI 的 stdout JSON 内容为准解析结果，而非以退出码为准：`status --json` 在 token 无效时仍输出完整 JSON 且以非零退出，插件 SHALL 解析该 JSON 并向模型展示真实状态（非零退出仅作状态提示，不视为调用失败）；`vision --json` 失败时以退出码非零 + `{"error"}` 判定失败。

插件 SHALL 在 apply 时探测 `visionary-server --version` 并记录版本号（探测失败按二进制缺失处理，不阻断插件加载）。版本探测 SHALL 仍只在 `apply()` 时执行一次——「二进制解析即时生效（懒解析）」仅适用于二进制**路径**解析，不改变版本探测时机。二进制版本与插件声明的兼容版本不匹配时，工具结果 SHALL 附带版本警告（不阻断调用），提示用户升级二进制或插件。

#### Scenario: 二进制缺失
- **WHEN** 插件已安装但 `visionary-server` 不在 PATH、未配置 `binaryPath` 且未设置 `DEEPSEEK_VISIONARY_BIN`
- **THEN** 工具调用返回错误信息（含平台对应的安装指引，Windows 下为 npm 指引，见「错误提示平台化」），DSH 进程不受影响

#### Scenario: 版本不匹配警告
- **WHEN** 插件探测到二进制版本与插件声明的兼容版本不一致
- **THEN** 工具结果附带版本警告（提示升级），调用本身不被阻断

#### Scenario: 未登录
- **WHEN** 模型调用 `deepseek_vision` 且未配置 token
- **THEN** 返回登录指引（调用 `deepseek_vision_login` 或设置 `DEEPSEEK_USER_TOKEN`）

#### Scenario: status 未登录仍返回状态 JSON
- **WHEN** 模型调用 `deepseek_vision_status` 且 token 未配置或无效（CLI 以非零退出但 stdout 为完整 JSON）
- **THEN** 插件解析 stdout JSON 并返回 authenticated: false / token_valid: false 的真实状态与登录指引，不因退出码非零而丢弃结果

### Requirement: Windows npm 全局安装场景下自动定位二进制真身
`npm install -g @xlight-oss/visionary-server` 在 Windows 的 PATH 中只生成 `.cmd` / `.ps1` shim（node 包装），真实 exe 位于包内 `node_modules/.bin_real/`。插件的二进制解析 SHALL 在 PATH 扫描 `.exe` 失败后，解析 shim 定位 exe 真身并 spawn 真身（保持 stdout 管道与 kill 链路完好）。

#### Scenario: npm 全局安装后插件可用
- **WHEN** Windows 上 `npm install -g @xlight-oss/visionary-server` 完成（PATH 只有 shim），未配置 `binaryPath` / `DEEPSEEK_VISIONARY_BIN`，调用 `deepseek_vision`
- **THEN** 插件从 `visionary-server.cmd` / `.ps1` shim 解析出 `node_modules\.bin_real\visionary-server.exe` 真身并执行，返回分析结果

#### Scenario: shim 缺失或格式异常时回退友好错误
- **WHEN** 无法从 PATH 找到 `.exe` 或可解析的 shim
- **THEN** 工具返回平台化的安装指引错误（win32 给出 npm / binaryPath 指引），不崩溃

### Requirement: 二进制解析即时生效（懒解析）
插件 SHALL 在每次工具调用时重新解析二进制路径（而非 `apply()` 时缓存一次），使用户修改 PATH 或设置 `DEEPSEEK_VISIONARY_BIN` 后无需重启 DSH 即生效。

#### Scenario: 设置环境变量后立即生效
- **WHEN** 工具首次调用因找不到二进制而失败，随后用户设置 `DEEPSEEK_VISIONARY_BIN` 指向有效 exe，再次调用同一工具
- **THEN** 第二次调用成功，无需重启 DSH

### Requirement: 错误提示平台化
二进制缺失的提示信息 SHALL 按平台给出对应安装命令：win32 给 npm 全局安装 / binaryPath 指引；其他平台给 curl / brew / npm 命令。

#### Scenario: Windows 用户看到 Windows 安装指引
- **WHEN** win32 平台二进制缺失
- **THEN** 提示包含 `npm install -g @xlight-oss/visionary-server`、`Config.binaryPath`、`DEEPSEEK_VISIONARY_BIN` 指引，不含 Unix 专属命令

### Requirement: 宿主级执行与会话语义
插件工具 SHALL 在 DSH 宿主进程内 spawn 子进程执行（不经 bash 沙箱），因此对 `~/.deepseek-visionary/` 的写入（`config.json`、`session.json`）与浏览器启动不受 DSH 文件沙箱（`workspace-write`）限制。`deepseek_vision` 的 `continue_conversation` / `session_id` 续聊语义 SHALL 与 MCP/CLI 一致：复用 `~/.deepseek-visionary/session.json` 持久化会话。

#### Scenario: 插件路径续聊不受沙箱限制
- **WHEN** DSH 会话处于 `workspace-write` 沙箱模式，模型连续两次调用 `deepseek_vision`（第二次 `continue_conversation=true`）
- **THEN** 两次调用都能读写 `~/.deepseek-visionary/session.json`（无需 danger-full-access），第二次调用复用第一次的会话

#### Scenario: login 在 DSH 会话内可执行
- **WHEN** 模型调用 `deepseek_vision_login`
- **THEN** 插件启动浏览器自动登录并写入 `~/.deepseek-visionary/config.json`，不受 bash 沙箱写限制

### Requirement: 系统提示整合
插件 SHALL 通过 `ctx.systemPrompt.section`（如可用）注入简短引导，说明 5 个原生工具的存在与使用场景（图片 / 截图 / 文档识图时优先调用 `deepseek_vision`；纯文字提取场景调用 `deepseek_ocr`），引导文本 SHALL 保持精简（不重复工具 schema 描述）。若用户环境同时装有 `visionary-cli` skill（如经 `init dsh` 安装），引导文本 SHALL 说明优先使用原生工具而非经 shell 调用 CLI（原生工具在宿主进程执行，会话续聊与登录不受 bash 沙箱限制）。

#### Scenario: 模型感知视觉能力
- **WHEN** 插件已加载且 DSH 会话开始
- **THEN** 系统提示包含视觉/OCR 工具引导段落，模型在用户提供图片时倾向调用 `deepseek_vision` 或 `deepseek_ocr` 而非猜测

#### Scenario: 并存时优先原生工具
- **WHEN** 用户同时装有 `visionary-cli` skill 与插件（原生工具）
- **THEN** 系统提示引导模型优先调用原生 `deepseek_vision` / `deepseek_ocr` 工具，而非经 bash 执行 `visionary-server`

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
# dsh-plugin Specification

## Purpose
定义 DeepSeek Visionary 的 DeepSeek Harness 原生插件包（npm 分发，纯 ESM 无构建）：经 `dsh.bundle.patch` 自注册的 Cordis bundle，向 `ctx.tools` 注册 `deepseek_vision` 等原生工具，工具通过 spawn `visionary-server` CLI（vision/status 经 `--json` 原子输出；login/logout 按子命令参数面透出文本）复用 Rust 视觉管道，提供结构化参数 schema 与宿主级权限（不经 bash 沙箱）。

## ADDED Requirements

### Requirement: 插件包结构与安装

插件包 SHALL 是一个标准 DSH bundle，满足以下结构要求：

- `package.json` 声明 `dsh.bundle.patch` 指向自带 `cordis.patch.yml`，使 `dsh plugin --profile <name> add <pkg>` 安装后自动追加到 profile 的 `dsh.profile.bundles` 层叠，用户无需手写任何配置
- `cordis.patch.yml` SHALL 以 `- insert:` 形式注册插件行（`id` + `name` 指向包名）
- `lib/index.mjs` SHALL 导出 Cordis 插件契约：`name`（与 patch 行 `id` 一致）、`inject`（含 `tools`）、`Config`（schemastery schema）、`apply(ctx, config)`
- 包 SHALL 为纯 ESM 普通 JavaScript，无构建步骤；`peerDependencies` 声明 `@deepseek-ai/cordis` 与 `@deepseek-ai/dsh-tools`（由 DSH profile 提供）
- 包名 SHALL 为 `@xlight-oss/visionary-dsh`（与既有 npm 通道 `@xlight-oss/visionary-server` 同 scope）

安装方式 SHALL 支持：`dsh plugin --profile web add @xlight-oss/visionary-dsh`（npm）与本地路径安装（`dsh plugin --profile web add <path>`）。

#### Scenario: 一键安装插件
- **WHEN** 用户执行 `dsh plugin --profile web add @xlight-oss/visionary-dsh` 并重启 DSH
- **THEN** 插件行出现在组合配置中，`ctx.tools` 注册 `deepseek_vision` 等工具，模型可调用

#### Scenario: bundle 自注册
- **WHEN** 检查安装后的 profile 组合配置（`dsh --profile web --dump-config`）
- **THEN** 出现 `@xlight-oss/visionary-dsh` 的 bundle 层，含 `visionary-vision` 插件行，无需用户编辑 cordis.patch.yml

### Requirement: 原生工具面

插件 SHALL 经 `ctx.tools.register(defineTool(...))` 注册 4 个原生工具，命名与 MCP 工具一致（`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`），参数 schema 与对应 MCP 工具对齐：

- `deepseek_vision`：`image`（必填，本地路径 / base64 / data URI）、`prompt`、`thinking`、`continue_conversation`、`session_id`
- `deepseek_vision_status`：无参数，输出登录状态
- `deepseek_vision_login`：浏览器自动登录（阻塞等待，超时可配置）
- `deepseek_vision_logout`：清除保存的凭据

工具 SHALL 按各子命令的真实参数面 spawn `visionary-server`：
- `deepseek_vision` SHALL spawn `visionary-server vision <image> --json` 并解析原子 JSON，返回 `{"text", "session_id", "parent_message_id"}` 的文本投影并在结果中携带会话信息
- `deepseek_vision_status` SHALL spawn `visionary-server status --json` 并解析原子 JSON
- `deepseek_vision_login` / `deepseek_vision_logout` SHALL spawn `visionary-server login` / `visionary-server logout`（**不带** `--json`——这两个子命令无该参数），直接透出文本输出；`login` 的阻塞时长由 `timeoutMs`（决策 10）兜底

选项传参 SHALL 一律使用等号形式（`--prompt=<value>` / `--session-id=<value>`），不用空格分隔——CLI（clap）将 `-` 开头的空格形式值当作 flag 拒绝。

#### Scenario: 识图
- **WHEN** 模型调用 `deepseek_vision` 传入图片路径与问题
- **THEN** 插件 spawn `visionary-server vision <image> --json --prompt=<q>`（等号传参），返回视觉模型回答文本与 session_id，工具调用成功

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

插件 SHALL 按以下优先级解析 `visionary-server` 二进制：`Config.binaryPath`（显式配置）→ `DEEPSEEK_VISIONARY_BIN` 环境变量 → PATH 查找。解析失败或二进制缺失时，工具调用 SHALL 返回清晰错误（含安装指引），不崩溃。

未登录时 `deepseek_vision` SHALL 返回登录指引（提示调用 `deepseek_vision_login` 或注入 `DEEPSEEK_USER_TOKEN`）。`vision` 失败（非零退出码）时 SHALL 透出 `{"error"}` 中的信息；`login` 超时（`Config.loginTimeoutSeconds`，默认读取 `DEEPSEEK_LOGIN_TIMEOUT` env、未设置时 600 秒）SHALL 返回超时错误。

工具 SHALL 以 CLI 的 stdout JSON 内容为准解析结果，而非以退出码为准：`status --json` 在 token 无效时仍输出完整 JSON 且以非零退出，插件 SHALL 解析该 JSON 并向模型展示真实状态（非零退出仅作状态提示，不视为调用失败）；`vision --json` 失败时以退出码非零 + `{"error"}` 判定失败。

插件 SHALL 在 apply 时探测 `visionary-server --version` 并记录版本号（探测失败按二进制缺失处理，不阻断插件加载）。二进制版本与插件声明的兼容版本不匹配时，工具结果 SHALL 附带版本警告（不阻断调用），提示用户升级二进制或插件。

#### Scenario: 二进制缺失
- **WHEN** 插件已安装但 `visionary-server` 不在 PATH、未配置 `binaryPath` 且未设置 `DEEPSEEK_VISIONARY_BIN`
- **THEN** 工具调用返回错误信息（含 install.sh / brew / npm 安装指引），DSH 进程不受影响

#### Scenario: 版本不匹配警告
- **WHEN** 插件探测到二进制版本与插件声明的兼容版本不一致
- **THEN** 工具结果附带版本警告（提示升级），调用本身不被阻断

#### Scenario: 未登录
- **WHEN** 模型调用 `deepseek_vision` 且未配置 token
- **THEN** 返回登录指引（调用 `deepseek_vision_login` 或设置 `DEEPSEEK_USER_TOKEN`）

#### Scenario: status 未登录仍返回状态 JSON
- **WHEN** 模型调用 `deepseek_vision_status` 且 token 未配置或无效（CLI 以非零退出但 stdout 为完整 JSON）
- **THEN** 插件解析 stdout JSON 并返回 authenticated: false / token_valid: false 的真实状态与登录指引，不因退出码非零而丢弃结果

### Requirement: 宿主级执行与会话语义

插件工具 SHALL 在 DSH 宿主进程内 spawn 子进程执行（不经 bash 沙箱），因此对 `~/.deepseek-visionary/` 的写入（`config.json`、`session.json`）与浏览器启动不受 DSH 文件沙箱（`workspace-write`）限制。`deepseek_vision` 的 `continue_conversation` / `session_id` 续聊语义 SHALL 与 MCP/CLI 一致：复用 `~/.deepseek-visionary/session.json` 持久化会话。

#### Scenario: 插件路径续聊不受沙箱限制
- **WHEN** DSH 会话处于 `workspace-write` 沙箱模式，模型连续两次调用 `deepseek_vision`（第二次 `continue_conversation=true`）
- **THEN** 两次调用都能读写 `~/.deepseek-visionary/session.json`（无需 danger-full-access），第二次调用复用第一次的会话

#### Scenario: login 在 DSH 会话内可执行
- **WHEN** 模型调用 `deepseek_vision_login`
- **THEN** 插件启动浏览器自动登录并写入 `~/.deepseek-visionary/config.json`，不受 bash 沙箱写限制

### Requirement: 系统提示整合

插件 SHALL 通过 `ctx.systemPrompt.section`（如可用）注入简短引导，说明 4 个原生工具的存在与使用场景（图片 / 截图 / 文档识图时优先调用 `deepseek_vision`），引导文本 SHALL 保持精简（不重复工具 schema 描述）。若用户环境同时装有 `visionary-cli` skill（如经 `init dsh` 安装），引导文本 SHALL 说明优先使用原生工具而非经 shell 调用 CLI（原生工具在宿主进程执行，会话续聊与登录不受 bash 沙箱限制）。

#### Scenario: 模型感知视觉能力
- **WHEN** 插件已加载且 DSH 会话开始
- **THEN** 系统提示包含视觉工具引导段落，模型在用户提供图片时倾向调用 `deepseek_vision` 而非猜测

#### Scenario: 并存时优先原生工具
- **WHEN** 用户同时装有 `visionary-cli` skill 与插件（原生工具）
- **THEN** 系统提示引导模型优先调用原生 `deepseek_vision` 工具，而非经 bash 执行 `visionary-server vision`

# cli-commands Specification

## Purpose
定义 `visionary-server` 的 CLI 子命令面：`vision` / `status` / `login` / `logout` 四个子命令，以命令行方式覆盖与 MCP 工具等价的核心能力，支持流式输出与 `--json` 结构化输出，供终端交互与脚本/agent 消费。
## Requirements
### Requirement: vision 子命令
`visionary-server` SHALL 提供 `vision` 子命令，以 CLI 方式运行 vision 流水线（上传 → completion，fork 已从服务端移除；上传携带 `x-model-type: vision` 直接完成多模态图像理解）。参数 SHALL 对齐 `deepseek_vision` 工具：`image` 位置参数（支持本地路径 / base64 / data URI / `-` 读 stdin）、`--prompt`、`--thinking`、`--continue`、`--session-id`、`--model-type <vision|ocr>`（默认 vision），另含 CLI 专属的 `--stream` / `--no-stream` / `--json`（见输出模式与 `--json` 需求）。`--session-id` 的优先级 SHALL 高于 `--continue`（与 MCP 工具一致）。未配置 token 时 SHALL 输出登录指引并退出非零。

#### Scenario: 分析本地图片
- **WHEN** 用户执行 `visionary-server vision /path/to/image.png`
- **THEN** 程序运行完整流水线（vision 管道），将视觉模型的回答输出到 stdout，退出码为 0

#### Scenario: 自定义问题与 DeepThink
- **WHEN** 用户执行 `visionary-server vision img.png --prompt "图中有什么？" --thinking`
- **THEN** 流水线携带指定 prompt 并启用 DeepThink，回答输出到 stdout

#### Scenario: 从 stdin 读图
- **WHEN** 用户执行 `cat img.png | visionary-server vision -`
- **THEN** 程序从 stdin 读取全部字节作为图片数据并完成分析

#### Scenario: base64 图片
- **WHEN** 用户以 base64 编码或 data URI 形式传入 `image`
- **THEN** 程序解码后完成分析并返回结果

#### Scenario: 未登录
- **WHEN** 执行 `vision` 时未配置有效 token
- **THEN** 非 `--json` 模式在 stderr 输出登录指引（提示运行 `visionary-server login` 或手动配置）并退出非零；`--json` 模式在 stdout 输出单个 `{"error": ...}` JSON 文档并退出非零（保持 JSON 原子性）

#### Scenario: 会话续聊
- **WHEN** 用户执行 `visionary-server vision img2.png --continue`
- **THEN** 程序复用持久化会话与父消息链继续对话，输出与 MCP 工具一致的续聊提示

#### Scenario: 显式切换会话
- **WHEN** 用户执行 `visionary-server vision img.png --session-id <id>`
- **THEN** 程序在该会话下发起新消息（无本地记录时仅复用 session_id），输出会话提示

#### Scenario: 指定 ocr 管道
- **WHEN** 用户执行 `visionary-server vision img.png --model-type ocr`
- **THEN** 程序走 OCR 管道（上传不携带 `x-model-type`）并返回提取的文字内容

### Requirement: vision 输出模式（TTY 感知）
`vision` 子命令的输出模式 SHALL 取决于 stdout 是否为 TTY 与显式开关：stdout 为 TTY 时 SHALL 流式打印 completion text 到 stdout（收到内容增量即打印，而非等待完整结果）；stdout 非 TTY 时 SHALL 原子输出完整文本。`--stream` SHALL 强制流式，`--no-stream` SHALL 强制原子输出，两者 SHALL 覆盖 TTY 检测默认。`--stream` 与 `--no-stream` 同时指定 SHALL 报错退出非零。进度日志 SHALL 走 stderr 不污染 stdout；输出结束后 SHALL 打印会话提示（`[session_id: xxx]`）。

#### Scenario: TTY 默认流式打印
- **WHEN** 用户在终端（stdout 为 TTY）执行 `visionary-server vision img.png`
- **THEN** completion 内容增量到达时即打印到 stdout，结束后附带 session_id 提示

#### Scenario: 非 TTY 默认原子输出
- **WHEN** stdout 非 TTY（如管道或 agent 捕获）且未指定开关，执行 `visionary-server vision img.png`
- **THEN** 程序一次性输出完整文本与结尾 session 提示，不输出流式增量

#### Scenario: 强制流式开关
- **WHEN** stdout 非 TTY 但用户执行 `visionary-server vision img.png --stream`
- **THEN** 程序强制流式打印内容增量到 stdout

#### Scenario: 强制原子开关
- **WHEN** stdout 为 TTY 但用户执行 `visionary-server vision img.png --no-stream`
- **THEN** 程序一次性输出完整文本，不流式打印

#### Scenario: 开关冲突
- **WHEN** 用户执行 `visionary-server vision img.png --stream --no-stream`
- **THEN** 程序在 stderr 报参数冲突错误，退出码非零

#### Scenario: 日志不污染 stdout
- **WHEN** 执行 `vision` 且开启日志
- **THEN** 所有日志与进度输出写入 stderr，stdout 仅包含回答文本与结尾会话提示

### Requirement: vision --json 输出
`vision` 子命令在 `--json` 模式下 SHALL 禁用流式，收集完整结果后一次性输出结构化 JSON 到 stdout。成功时 SHALL 输出 `{"text": ..., "session_id": ..., "parent_message_id": ...}`；失败时 SHALL 输出 `{"error": ...}` 且退出非零。JSON 输出 SHALL 是原子的（单一完整 JSON 文档，无流式内容混入）。

#### Scenario: 成功输出 JSON
- **WHEN** 用户执行 `visionary-server vision img.png --json` 且分析成功
- **THEN** stdout 输出单个 JSON 文档，包含 text、session_id、parent_message_id 字段，退出码为 0

#### Scenario: 失败输出 JSON
- **WHEN** 用户执行 `visionary-server vision missing.png --json`（图片不存在）
- **THEN** stdout 输出单个 `{"error": ...}` JSON 文档，退出码非零

#### Scenario: json 与流式开关冲突
- **WHEN** 用户执行 `visionary-server vision img.png --json --stream`
- **THEN** 程序在 stderr 报参数冲突错误，退出码非零（`--json` 恒为原子输出，不与流式开关组合）

### Requirement: status 子命令
`visionary-server` SHALL 提供 `status` 子命令，输出轻量鉴权状态：token 是否配置、smidV2 cookie、Base URL，并通过真实校验探针验证 token 有效性。默认输出 SHALL 为可读文本到 stdout；`status --json` SHALL 输出原子 JSON `{"authenticated", "token_configured", "smid_v2", "base_url", "token_valid"}`。token 未配置或探针失败时 SHALL 提示重新登录并退出非零（无论是否 `--json`，`--json` 模式下 stdout 仍输出完整状态 JSON 含 `token_valid: false`）。

#### Scenario: token 有效
- **WHEN** 用户执行 `visionary-server status` 且已配置有效 token
- **THEN** stdout 显示已认证与探针校验通过，退出码为 0

#### Scenario: token 失效
- **WHEN** 用户执行 `visionary-server status` 且 token 缺失或校验失败
- **THEN** stdout 显示未认证并提示运行 `visionary-server login`，退出码非零

#### Scenario: status --json 有效
- **WHEN** 用户执行 `visionary-server status --json` 且 token 有效
- **THEN** stdout 输出单个 JSON 文档，包含 authenticated / token_configured / smid_v2 / base_url / token_valid 字段（token_valid 为 true），退出码为 0

#### Scenario: status --json 失效
- **WHEN** 用户执行 `visionary-server status --json` 且 token 缺失或校验失败
- **THEN** stdout 输出单个 JSON 文档（token_valid 为 false），退出码非零，调用方可凭退出码判断可用性

### Requirement: login 子命令
`visionary-server` SHALL 提供 `login` 子命令，复用 CDP 浏览器自动登录流程（打开浏览器 → 等待登录 → 抓取凭据 → 保存）。成功时 SHALL 输出凭据摘要（脱敏）；已配置 token 时 SHALL 幂等提示不重复登录；超时或失败时 SHALL 输出错误与继续路径（浏览器保持打开），退出码非零。

#### Scenario: 登录成功
- **WHEN** 用户执行 `visionary-server login` 并在浏览器中完成登录
- **THEN** 程序抓取并保存凭据，stdout 输出脱敏凭据摘要，退出码为 0

#### Scenario: 已配置 token
- **WHEN** 用户执行 `visionary-server login` 且已配置 token
- **THEN** 程序提示已配置、不重复登录，退出码为 0

#### Scenario: 登录超时
- **WHEN** 用户在等待超时内未完成登录
- **THEN** 程序输出错误提示（浏览器保持打开，可重跑 `visionary-server login`），退出码非零

### Requirement: logout 子命令
`visionary-server` SHALL 提供 `logout` 子命令，清除保存的凭据并输出结果。成功时退出码为 0。

#### Scenario: 清除凭据
- **WHEN** 用户执行 `visionary-server logout`
- **THEN** 程序清除保存的凭据，stdout 输出清除结果，退出码为 0

### Requirement: 子命令失败退出码
所有 CLI 子命令（`vision` / `status` / `login` / `logout`）SHALL 在成功时退出码为 0，失败时（未登录、图片读取失败、流水线错误、登录失败等）退出码非零，错误信息 SHALL 输出到 stderr。CLI 子命令执行 SHALL 不进入 MCP serve 模式。

#### Scenario: 未知子命令
- **WHEN** 用户传入不存在的子命令（如 `visionary-server frobnicate`）
- **THEN** 程序在 stderr 输出错误与用法提示，退出码非零，不进入 serve 模式

#### Scenario: 参数缺失
- **WHEN** 用户执行 `visionary-server vision`（缺 `image` 位置参数）
- **THEN** 程序输出用法错误到 stderr，退出码非零

### Requirement: agent 调用契约
项目 SHALL 提供 agent 调用契约文档（`skills/visionary-cli/SKILL.md`），描述 `vision` 子命令的用法、`--json` 输出形状与退出码约定。契约 SHALL 规定 agent 调用 `vision` 时使用 `--json` 原子输出（而非解析流式文本），并说明非 TTY 环境默认原子输出的兑底行为。

`skill` 子命令 SHALL 将内嵌于二进制的 SKILL.md（`include_str!`）写入 `~/.agents/skills/visionary-cli/SKILL.md`（自动创建目录），使通过安装脚本装二进制的用户无需仓库即可获得 skill。已存在时 SHALL 覆盖并提示。写入失败时 SHALL 输出错误并退出非零。

skill 的安装逻辑 SHALL 抽为可复用函数（输入目标技能根目录，输出写入路径），供 `init dsh` 复用：`init dsh` SHALL 经同一函数将内嵌 SKILL.md 安装到 `$DSH_HOME/skills/visionary-cli/SKILL.md`（DSH user 技能根）与 `~/.agents/skills/visionary-cli/SKILL.md`。`skill install` 自身行为不变（仍只写 `~/.agents/skills/visionary-cli/SKILL.md`）。

#### Scenario: agent 通过 skill 调用
- **WHEN** agent 读取 `skills/visionary-cli/SKILL.md` 后调用 `visionary-server vision img.png --json`
- **THEN** agent 获得原子 JSON 输出（含 text / session_id / parent_message_id），可可靠解析

#### Scenario: skill install 安装
- **WHEN** 用户执行 `visionary-server skill install`
- **THEN** 程序将内嵌 SKILL.md 写入 `~/.agents/skills/visionary-cli/SKILL.md`（自动创建目录），输出安装路径，退出码为 0

#### Scenario: skill install 覆盖既有文件
- **WHEN** 用户执行 `visionary-server skill install` 且目标文件已存在
- **THEN** 程序覆盖写入并提示已更新，退出码为 0

#### Scenario: init dsh 复用 skill 安装逻辑
- **WHEN** 用户执行 `visionary-server init dsh`
- **THEN** 程序经与 `skill install` 相同的安装函数将内嵌 SKILL.md 写入 `$DSH_HOME/skills/visionary-cli/SKILL.md` 与 `~/.agents/skills/visionary-cli/SKILL.md`，两处文件内容与内嵌一致

### Requirement: ocr 子命令
`visionary-server` SHALL 提供 `ocr` 子命令，以 CLI 方式运行 OCR 文本提取（等价于 `vision --model-type ocr`），参数面 SHALL 对齐 `vision` 子命令（`image` 位置参数、`--prompt`、`--thinking`、`--continue`、`--session-id`、`--stream` / `--no-stream` / `--json`）。未配置 token 时 SHALL 输出登录指引并退出非零。

#### Scenario: 提取文档文字
- **WHEN** 用户执行 `visionary-server ocr /path/to/doc.png`
- **THEN** 程序运行 OCR 管道，将提取的文字输出到 stdout，退出码为 0

#### Scenario: 无文字图片
- **WHEN** 用户执行 `visionary-server ocr img.png` 且图片文字不足（服务端返回 `CONTENT_EMPTY`）
- **THEN** 程序输出"图片中未提取到文字"提示，退出码非零

#### Scenario: JSON 输出
- **WHEN** 用户执行 `visionary-server ocr img.png --json`
- **THEN** 程序输出原子 JSON（`{"text", "session_id", "parent_message_id"}`），与 `vision --json` 形状一致


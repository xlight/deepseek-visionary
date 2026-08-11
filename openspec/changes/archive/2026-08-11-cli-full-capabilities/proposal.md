## Why

`visionary-server` 的核心能力（识图、状态检查、自动登录、退出登录）目前**只能通过 MCP 工具调用**：CLI 侧只有 `--version` / `doctor` / `init` 三个外围命令。用户无法在终端直接使用二进制完成识图、登录、查状态等操作——"完整能力只存在于 MCP 协议之后"，CLI 是残缺的。

## What Changes

- **新增 `vision` 子命令**：CLI 方式运行完整 vision 流水线（上传 → fork → HIF 签名 → completion）。参数对齐 `deepseek_vision` 工具：`image`（位置参数，支持路径 / base64 / data URI / `-` 读 stdin）、`--prompt`、`--thinking`、`--continue`、`--session-id`、`--json`
- **流式输出（TTY 感知）**：stdout 为 TTY 时默认流式打印 completion text（打字机效果）；stdout 非 TTY（管道/agent 捕获）时默认原子输出完整文本。提供 `--stream` / `--no-stream` 显式开关覆盖默认。进度日志继续走 stderr
- **`--json` 结构化输出**：禁用流式，一次性输出 `{"text", "session_id", "parent_message_id"}`（失败时输出 `{"error"}` 且退出非零），供脚本/agent 消费
- **agent 调用契约（CLI + SKILL）**：新增 `skills/visionary-cli/SKILL.md` 描述 CLI 用法与调用约定（agent 调用 MUST 使用 `--json` 原子输出，不解析流式文本），README 附安装说明——CLI 成为 agent 的零 MCP 配置工具面
- **新增 `status` 子命令**：轻量鉴权检查（对齐 `deepseek_vision_status` 工具：token 配置状态 + 真实探针），与 `doctor`（完整环境诊断）并存
- **新增 `login` 子命令**：复用现有 CDP 浏览器自动登录流程，提示文案从 MCP 措辞泛化为 CLI 措辞
- **新增 `logout` 子命令**：清除保存的凭据
- **核心层去 MCP 化重构**：`login.rs::run_login` / `run_logout` 从返回 `rmcp::CallToolResult` 改为返回普通类型，`server.rs` 包回 MCP 层；`deepseek_vision` handler 的会话续聊解析与图片读取逻辑抽为共享函数
- **失败行为**：所有子命令失败时退出非零（沿用 `doctor` 先例）

## Capabilities

### New Capabilities

- `cli-commands`: `visionary-server` 的 CLI 子命令面——`vision` / `status` / `login` / `logout`，覆盖与 MCP 工具等价的核心能力，支持 TTY 感知的流式输出、`--stream`/`--no-stream` 开关与 `--json` 结构化输出，并配套 agent 调用契约（SKILL）

### Modified Capabilities

<!-- 无需求级变更：vision-analysis（识图/流水线/会话续聊）与 auto-login（浏览器登录）的
     行为需求不变，本次仅新增 CLI 作为其访问入口；MCP 工具行为保持完全一致。 -->

## Impact

- **代码**：`crates/visionary-server/src/cli.rs`（新增 4 个子命令 + 参数解析 + TTY 检测）；`server.rs`（会话续聊解析与图片读取抽离，login/logout 返回类型适配）；`login.rs`（返回值去 MCP 化）；`completion.rs` / `pipeline.rs`（流式回调透传）
- **无新依赖**：clap / tokio / futures-util 均已存在；TTY 检测用标准库 `std::io::IsTerminal`
- **行为影响**：MCP 工具输出与行为不变；stdout 约束（MCP stdio 通道）不受影响，CLI 子命令与 serve 模式共用 stderr 日志基建
- **测试**：`cli.rs` 补 clap 解析测试（vision 各 flag 组合、`--stream`/`--no-stream`、stdin）；`completion.rs` 补流式分支单测；现有 `tests/cli.rs` 集成测试扩展
- **文档**：README CLI 工具表补 4 行；`docs/` 补 CLI 用法说明；新增 `skills/visionary-cli/SKILL.md`（agent 调用契约）
- **关联**：与并行 change `generalize-mcp-distribution`（引入 `server-cli` 能力，含 `--version`/`doctor`/`init`）互补——本 change 的 `cli-commands` 是其 CLI 能力的延伸，两者最终可合并归档

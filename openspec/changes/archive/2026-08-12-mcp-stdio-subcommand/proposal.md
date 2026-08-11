# mcp-stdio-subcommand

## Why

当前 `visionary-server` 无参数时直接进入 MCP stdio serve 模式，在终端直接运行时会让用户困惑（进程挂起等待 stdin）。项目定位转向"CLI 优先"：CLI + skill 是主推使用方式，MCP 是可选通道。因此无参数应输出 help 引导用户，MCP stdio 服务改为显式 `mcp-stdio` 子命令启动。

## What Changes

- **BREAKING** 无参数运行 `visionary-server` 从"MCP stdio serve"改为"输出 help 并退出码 2"（clap `arg_required_else_help`）
- **BREAKING** 新增 `mcp-stdio` 子命令，显式启动 MCP stdio 服务（替代原无参数行为）
- **BREAKING** 所有 agent 接入配置（opencode / codex / claude / claude-desktop / cursor）的启动命令从 `visionary-server` 改为 `visionary-server mcp-stdio`：
  - `init` 子命令写入的配置形状同步更新
  - Zed 扩展壳 `context_server_command` 的 args 增加 `mcp-stdio`
  - `mcp_probe.py` CI smoke 脚本、`build_mcpb.py` MCPB manifest 同步更新
- `skill install` 行为不变（无 action 默认 install）
- `--version` / `-V` 行为不变
- README 安装章节改为"CLI + skill 优先"，MCP 接入降级为进阶选项

## Capabilities

### New Capabilities

（无。`mcp-stdio` 命令与无参数 help 行为均属于 `server-cli` 能力面。）

### Modified Capabilities

- `server-cli`: "无参数启动进入 MCP serve 模式"需求改为"`mcp-stdio` 子命令启动 MCP stdio 服务 + 无参数输出 help（exit 2）"
- `agent-onboarding`: "按 agent 配置形状写入 MCP 配置"需求中所有启动命令形状加 `mcp-stdio` 参数；"接入文档矩阵"需求中 README 改为 CLI + skill 优先

## Impact

- `crates/visionary-server/src/cli.rs`：新增 `mcp-stdio` 子命令；无参数行为改为输出 help（clap `arg_required_else_help = true`）
- `crates/visionary-server/src/onboarding.rs`：所有 agent 配置写入逻辑（opencode / codex CLI / claude CLI / JSON / TOML）加 `mcp-stdio` 参数
- `crates/visionary-zed-ext/src/lib.rs`：`context_server_command` 返回的 args 增加 `mcp-stdio`
- `scripts/mcp_probe.py`：Popen 启动命令加 `mcp-stdio`
- `scripts/build_mcpb.py`：manifest `mcp_config.args` 改为 `["mcp-stdio"]`
- `crates/visionary-server/tests/cli.rs`：`no_args_starts_mcp_serve_and_handshakes` 改为用 `mcp-stdio` 启动；新增无参数输出 help 测试
- `src/cli.rs` 内部单元测试：`no_args_means_serve` 改为 `no_args_means_help`
- 文档：`README.md`、`docs/cli.md`、`docs/integrations/*.md`（opencode / codex / claude-code / claude-desktop / cursor / zed）
- Spec：`openspec/specs/server-cli/spec.md`、`openspec/specs/agent-onboarding/spec.md`
- 已部署的存量 agent 配置（无参启动）升级后需重新 `init` 或手动改为 `mcp-stdio` 参数

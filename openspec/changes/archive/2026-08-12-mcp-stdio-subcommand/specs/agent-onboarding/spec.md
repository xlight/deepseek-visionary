## MODIFIED Requirements

### Requirement: 按 agent 配置形状写入 MCP 配置
`visionary-server init <agent>` SHALL 按各 agent 的 MCP 配置格式写入对应配置，注册名为 `deepseek-visionary`，启动命令指向 `visionary-server mcp-stdio`（显式 MCP stdio 模式）。各 agent 形状 MUST 符合以下要求：

- **opencode**：写顶层 `mcp` 键，`type: "local"`，`command` 为数组 `["visionary-server", "mcp-stdio"]`，并显式设置 `timeout: 60000`（规避 npx 冷启动与首次启动超时）
- **Codex**：优先 `codex mcp add deepseek-visionary -- visionary-server mcp-stdio`（`--` 分隔符形式）；失败时写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]`（含 `command` 与 `args = ["mcp-stdio"]`）
- **Claude Code**：优先调用 `claude mcp add --transport stdio deepseek-visionary -- visionary-server mcp-stdio`；失败时写入用户级 MCP 配置的 `mcpServers` 形状（`args: ["mcp-stdio"]`）
- **Cursor**：写入 `mcpServers` 形状到 Cursor 的 MCP 配置（用户级或项目级 `.cursor/mcp.json`），`args: ["mcp-stdio"]`
- **Claude Desktop**：写入 `mcpServers` 形状到 Claude Desktop 的 `claude_desktop_config.json`，`args: ["mcp-stdio"]`

#### Scenario: 配置 opencode
- **WHEN** 用户执行 `visionary-server init opencode` 且 opencode 已安装
- **THEN** opencode 配置文件的顶层 `mcp` 键下写入 `deepseek-visionary` 条目，含 `type: "local"`、`command: ["visionary-server", "mcp-stdio"]`、`timeout: 60000`

#### Scenario: 配置 Codex
- **WHEN** 用户执行 `visionary-server init codex`
- **THEN** 优先调用 `codex mcp add deepseek-visionary -- visionary-server mcp-stdio` 注册；CLI 不可用时写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]` 段（`args = ["mcp-stdio"]`），注册 stdio 本地服务

#### Scenario: 配置 Claude Code
- **WHEN** 用户执行 `visionary-server init claude` 且 `claude` CLI 可用
- **THEN** 调用 `claude mcp add` 注册服务（命令带 `mcp-stdio` 参数）；若 CLI 不可用则直接写入用户级配置文件的 `mcpServers` 形状（`args: ["mcp-stdio"]`）

#### Scenario: 配置 Cursor
- **WHEN** 用户执行 `visionary-server init cursor`
- **THEN** 在 Cursor 的 MCP 配置中写入 `mcpServers` 下的 `deepseek-visionary` 条目（`command` + `args: ["mcp-stdio"]` 形式）

#### Scenario: 配置 Claude Desktop
- **WHEN** 用户执行 `visionary-server init claude-desktop`
- **THEN** 在 `claude_desktop_config.json` 的 `mcpServers` 下写入 `deepseek-visionary` 条目（`args: ["mcp-stdio"]`）

### Requirement: 接入文档矩阵
仓库 SHALL 提供 `docs/integrations/` 目录，为每个受支持 agent（opencode、Codex、Claude Code、Claude Desktop、Cursor、Zed）提供一份 Markdown 接入文档。每份文档 MUST 包含：接入方式、手动配置示例、常见问题（如首次启动超时、登录与环境变量）。所有 MCP 配置示例 SHALL 使用 `visionary-server mcp-stdio` 启动命令。opencode / Codex / Claude Code / Claude Desktop / Cursor 文档以 `visionary-server init <agent>` 作为一键接入方式；Zed 文档 SHALL 以扩展市场安装为接入方式（Zed 无 MCP 配置文件，不在 init 支持范围）。README 的安装章节 SHALL 以 **CLI + skill 为优先推荐方式**（`visionary-server skill install` + 直接调用 `vision` 子命令），MCP 接入（init / Zed 扩展）作为进阶选项。README 或文档 SHALL 提及新兴安装通道 `apm install --mcp io.github.xlight/deepseek-visionary`（Microsoft APM，复用 MCP Registry 标识，无需额外建设）。

#### Scenario: 文档覆盖全部受支持 agent
- **WHEN** 检查 `docs/integrations/` 目录
- **THEN** 存在 opencode.md、codex.md、claude-code.md、claude-desktop.md、cursor.md、zed.md 六份文档，每份均含一键接入与手动配置两种路径，且所有配置示例使用 `mcp-stdio` 参数

#### Scenario: README 指引
- **WHEN** 用户阅读 README 安装章节
- **THEN** 首先看到"CLI + skill"优先使用方式（`visionary-server skill install` + `vision` 子命令），MCP 接入（`init` / Zed 扩展）作为进阶选项，并链接到 `docs/integrations/`

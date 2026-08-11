# Codex 接入

在 OpenAI Codex 中使用 DeepSeek Visionary MCP server。

## 一键接入（推荐）

```bash
visionary-server init codex
```

优先调用 `codex mcp add` 注册；若 `codex` CLI 不可用，则回退写入
`~/.codex/config.toml`。

## 手动配置

### 方式 A：CLI（推荐）

```bash
codex mcp add deepseek-visionary -- visionary-server mcp-stdio
```

> 注意 `--` 分隔符形式：`codex mcp add <name> -- <command> [args...]`。
> 旧文档中的 `--command` flag 形式已过时。

### 方式 B：直接编辑 config.toml

在 `~/.codex/config.toml` 中添加：

```toml
[mcp_servers.deepseek-visionary]
command = "visionary-server"
args = ["mcp-stdio"]
```

> **键名必须是 `mcp_servers`**（不是 `mcp.servers`）。`mcp.servers` 会被 Codex
> 静默忽略（GitHub issue #3441），配置看起来生效实则无效。

## 验证

```bash
codex mcp list
```

或直接问 Codex "你能用 deepseek_vision 工具吗"。

## 常见问题

- **配置后无工具**：检查键名是否为 `mcp_servers`（`mcp.servers` 静默失效）。
- **未登录**：调用 `deepseek_vision` 前先调用 `deepseek_vision_login`，或用环境变量 `DEEPSEEK_USER_TOKEN`。
- **二进制不在 PATH**：确认 `visionary-server` 在 PATH；或用绝对路径：
  ```toml
  [mcp_servers.deepseek-visionary]
  command = "/path/to/visionary-server"
  args = ["mcp-stdio"]
  ```

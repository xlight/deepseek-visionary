# Claude Code 接入

在 Claude Code 中使用 DeepSeek Visionary MCP server。

## 一键接入（推荐）

```bash
visionary-server init claude
```

优先调用 `claude mcp add` 注册到用户级配置；若 `claude` CLI 不可用，
则回退直接写入用户级 `~/.claude.json` 的 `mcpServers`。

## 手动配置

### 方式 A：CLI（推荐）

```bash
claude mcp add --transport stdio deepseek-visionary -- visionary-server
```

加 `--scope user` 写入用户级配置（`~/.claude.json`），而非项目级 `.mcp.json`：

```bash
claude mcp add --transport stdio deepseek-visionary --scope user -- visionary-server
```

### 方式 B：直接编辑配置

项目级 `.mcp.json` 或用户级 `~/.claude.json`：

```json
{
  "mcpServers": {
    "deepseek-visionary": {
      "command": "visionary-server",
      "args": []
    }
  }
}
```

## 验证

```bash
claude mcp list
```

## 常见问题

- **未登录**：调用 `deepseek_vision` 前先调用 `deepseek_vision_login`，或用环境变量 `DEEPSEEK_USER_TOKEN`。
- **二进制不在 PATH**：确认 `visionary-server` 在 PATH；或把 `command` 改为绝对路径。

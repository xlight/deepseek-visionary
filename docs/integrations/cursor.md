# Cursor 接入

在 Cursor 中使用 DeepSeek Visionary MCP server。

## 一键接入（推荐）

```bash
visionary-server init cursor
```

写入用户级 `~/.cursor/mcp.json` 的 `mcpServers` 键。

## 手动配置

### 用户级（所有项目）

编辑 `~/.cursor/mcp.json`：

```json
{
  "mcpServers": {
    "deepseek-visionary": {
      "command": "visionary-server",
      "args": ["mcp-stdio"]
    }
  }
}
```

### 项目级（仅当前项目）

在项目根目录创建 `.cursor/mcp.json`（同上形状）。

## 生效

- 打开 Cursor `Settings → Features → MCP`（或命令面板搜索 "MCP"）。
- 点击刷新按钮加载新服务；确认 `deepseek-visionary` 状态为已连接。

## 常见问题

- **看不到工具**：在 MCP 面板点击刷新；确认 `visionary-server` 在 PATH。
- **未登录**：调用 `deepseek_vision_login` 自动登录；或设置环境变量 `DEEPSEEK_USER_TOKEN`。
- **二进制不在 PATH**：把 `command` 改为绝对路径。

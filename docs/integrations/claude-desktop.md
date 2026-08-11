# Claude Desktop 接入

在 Claude Desktop 桌面应用中使用 DeepSeek Visionary MCP server。

> Claude Desktop 是桌面应用，无法调用终端命令来配置——只能手动编辑配置文件。

## 一键接入（macOS）

```bash
visionary-server init claude-desktop
```

## 手动配置

### macOS

编辑 `~/Library/Application Support/Claude/claude_desktop_config.json`：

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

### Windows

编辑 `%APPDATA%\Claude\claude_desktop_config.json`（同一 `mcpServers` 形状）。

## 生效

保存后**完全退出并重启** Claude Desktop（关闭菜单栏图标，不只是关窗口）。

## 常见问题

- **看不到工具**：确认已完全退出重启；确认 `visionary-server` 在 PATH（Windows 下为 `visionary-server.exe`）。
- **未登录**：在 Claude Desktop 中调用 `deepseek_vision_login` 完成浏览器自动登录；或预先设置环境变量 `DEEPSEEK_USER_TOKEN`（需从启动器以该环境变量启动应用）。
- **Mac 上 `command` 找不到**：用绝对路径，如 `"/usr/local/bin/visionary-server"`。

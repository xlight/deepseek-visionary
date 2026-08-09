# OpenCode 接入

在 OpenCode 中使用 DeepSeek Visionary MCP server。

## 一键接入（推荐）

```bash
visionary-server init opencode
```

该命令会向 `~/.config/opencode/opencode.json` 的顶层 `mcp` 键写入：

```json
{
  "mcp": {
    "deepseek-visionary": {
      "type": "local",
      "command": ["visionary-server"],
      "enabled": true,
      "timeout": 60000
    }
  }
}
```

> **为什么 `timeout: 60000`？** OpenCode 的 MCP 工具拉取超时默认 **5000ms**。
> `visionary-server` 是原生二进制、启动很快，但若通过 `npx`/网络安装器首次冷启动，
> 5 秒不够下载/解压，会导致"工具加载失败"。显式设 60000 规避该坑。

## 手动配置

编辑 `~/.config/opencode/opencode.json`（或项目级 `opencode.json`），加入：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "deepseek-visionary": {
      "type": "local",
      "command": ["visionary-server"],
      "enabled": true,
      "timeout": 60000
    }
  }
}
```

> OpenCode 只认顶层 `mcp` 键，不接受 Claude/Cursor 的 `mcpServers` 形状。
> `command` 必须是**数组**形式。

## 验证

```bash
opencode mcp list
```

应能看到 `deepseek-visionary`。之后在对话中即可调用 `deepseek_vision` 识图。

## 常见问题

- **首次启动超时 / 工具加载失败**：确认 `timeout` 已设为 60000；确认 `visionary-server` 在 PATH。
- **`command` 解析失败**：OpenCode 要求 `command` 为数组（`["visionary-server"]`），不是字符串。
- **未登录**：调用 `deepseek_vision` 前先调用 `deepseek_vision_login` 自动登录，或用环境变量 `DEEPSEEK_USER_TOKEN` 注入 token。
- **二进制不在 PATH**：`init` 会检测并提示安装方式；也可在 `command` 中写绝对路径。

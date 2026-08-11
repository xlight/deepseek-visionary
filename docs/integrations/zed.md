# Zed 接入

在 Zed 中使用 DeepSeek Visionary MCP server。

> Zed 的 MCP server 通过**扩展（extension）**接入，没有可手写的 MCP 配置文件。
> `visionary-server init` 不覆盖 Zed——请按下面方式安装扩展。

## 安装扩展

1. 打开 Zed 命令面板（`Cmd+Shift+P`），输入 **`zed: extensions`** 并回车
2. 搜索 **DeepSeek Visionary**
3. 点击 **Install**

扩展壳（`visionary-zed-ext`，wasm32-wasip2）会自动按平台从 GitHub Releases
下载/缓存 `visionary-server` 原生二进制，并以 `mcp-stdio` 参数启动 MCP 服务。

## 本地联调（开发模式）

在 Zed 中通过 `Extensions → Install Dev Extension` 选择
`crates/visionary-zed-ext` 目录；调试时可在扩展设置中配置 `server_path` 指向本地
构建的 `visionary-server`。

## 权限设置

在 Zed 设置中为扩展授予工具权限：

```json
{
  "agent": {
    "tool_permissions": {
      "context_servers.deepseek-visionary": {
        "deepseek_vision": "allow",
        "deepseek_vision_login": "allow",
        "deepseek_vision_status": "allow",
        "deepseek_vision_logout": "allow"
      }
    }
  }
}
```

## 使用

1. 调用 `deepseek_vision_login` 完成浏览器自动登录
2. 调用 `deepseek_vision` 识图

## 常见问题

- **扩展报错 `release ... has no asset`**：对应平台未发布二进制，等待新版 release。
- **未登录**：调用 `deepseek_vision_login`；或设置环境变量 `DEEPSEEK_USER_TOKEN`。

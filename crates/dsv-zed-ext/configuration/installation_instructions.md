# DeepSeek Vision MCP 安装说明

安装扩展后，首次使用前需要登录 DeepSeek 网页版获取 token：

## 方式一：自动登录（推荐）

在 Zed 中调用 MCP 工具 `deepseek_vision_login`：

1. 会打开一个浏览器窗口并导航到 chat.deepseek.com
2. 在浏览器中完成登录（支持扫码 / 密码 / 验证码）
3. 登录成功后工具自动抓取 token 与 cookies 并保存
4. 之后即可直接使用 `deepseek_vision` 识图

> 登录凭据保存在 `~/.deepseek-vision/config.json`（权限 0600）。
> 浏览器使用独立 profile（`~/.deepseek-vision/browser/`），不影响日常浏览器。

## 方式二：手动配置（兜底）

1. 打开 https://chat.deepseek.com 并登录
2. 按 F12 打开 DevTools → Application → Local Storage
3. 找到 `userToken`，复制 `JSON.parse(value).value` 的值
4. 编辑 `~/.deepseek-vision/config.json`：

```json
{
  "user_token": "粘贴你的 token",
  "smid_v2": "（可选）smidV2 cookie 值",
  "cf_clearance": "（可选）cf_clearance cookie 值"
}
```

## 使用

- `deepseek_vision`：上传本地图片（路径或 base64）并分析
- `deepseek_vision_status`：检查登录状态与 token 有效性
- `deepseek_vision_login` / `deepseek_vision_logout`：登录 / 登出

建议在 Zed 设置中为该扩展授予工具权限：

```json
{
  "agent": {
    "tool_permissions": {
      "context_servers.deepseek-vision": {
        "deepseek_vision": "allow",
        "deepseek_vision_login": "allow",
        "deepseek_vision_status": "allow",
        "deepseek_vision_logout": "allow"
      }
    }
  }
}
```

## 二进制下载

扩展会从 GitHub Releases 自动下载与当前平台匹配的 `dsv-server` 二进制
（macOS / Linux / Windows，arm64 / x86_64），按版本缓存。也可以设置
`server_path` 指向本地构建的二进制（开发调试用）。

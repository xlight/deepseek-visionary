---
name: visionary-cli
description: Use the visionary-server CLI to analyze images with DeepSeek's web-native vision model, check auth status, and manage login. Use this whenever the user provides an image, photo, screenshot, or document with images, or needs to check/repair DeepSeek vision login state.
license: MIT
metadata:
  author: deepseek-visionary
  version: "1.0"
---

# DeepSeek Visionary CLI

`visionary-server` 是 DeepSeek 网页版原生多模态视觉模型的 CLI 入口（也是 MCP server）。本 skill 描述 agent 如何通过命令行调用其全部能力。

## 前置条件

- `visionary-server` 二进制在 PATH 中（`visionary-server --version` 可验证）
- 需要已登录：`visionary-server status` 退出码为 0 且 `token_valid` 为 true

## 调用契约（重要）

**agent 调用 `vision` 时 MUST 使用 `--json` 原子输出，不要依赖流式文本。**

原因：agent 以 spawn 子进程 + 捕获 stdout 的方式调用，进程退出后才能拿到完整输出；流式文本是给人看的打字机效果，session_id 等元信息是结尾拼接的文本，无结构化边界，无法可靠解析。`--json` 输出是单一原子 JSON 文档。

兜底：即使忘记 `--json`，非 TTY 环境（agent 捕获 stdout）默认也是原子文本输出，不会收到流式垃圾；但结构化信息仍需 `--json`。

## 命令

### 1. 分析图片

```
visionary-server vision <image> [--prompt <问题>] [--thinking] [--continue] [--session-id <id>] [--json]
```

- `<image>`：本地路径 / base64 / data URI / `-`（从 stdin 读）
- `--json`：**必须**。成功输出：

```json
{
  "text": "图片内容描述……",
  "session_id": "abc123",
  "parent_message_id": "msg456"
}
```

- 失败输出（退出码非零）：

```json
{ "error": "错误信息" }
```

- `--thinking` 启用 DeepThink 深度思考（回答前多步推理）
- 多图对比：第一次不带续聊参数，之后对每张新图加 `--continue`（复用上次会话）或 `--session-id <上一轮的 session_id>`

示例：

```bash
visionary-server vision /path/to/screenshot.png --json
visionary-server vision /path/to/photo.png --json --prompt "图中有什么？" --thinking
cat img.png | visionary-server vision - --json
visionary-server vision img2.png --json --continue
```

### 2. 状态检查（登录前预检）

```
visionary-server status --json
```

```json
{
  "authenticated": true,
  "token_configured": true,
  "smid_v2": true,
  "base_url": "https://chat.deepseek.com",
  "token_valid": true
}
```

- `token_valid: false` 或退出码非零 → 需要登录，先运行 `login`
- 调用 `vision` 前建议先 `status --json` 预检

### 3. 登录

```
visionary-server login
```

打开浏览器自动登录（阻塞等待，超时默认 600 秒，可用 `DEEPSEEK_LOGIN_TIMEOUT` 覆盖）。成功输出脱敏凭据摘要；失败退出非零（浏览器保持打开可重试）。

### 4. 退出登录

```
visionary-server logout
```

清除保存的凭据。

## 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 成功 |
| 1 | 失败（未登录 / 图片读取失败 / 流水线错误 / 登录失败） |
| 2 | 参数错误 |

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_USER_TOKEN` | 覆盖 token（可选） |
| `DEEPSEEK_LOGIN_TIMEOUT` | 登录等待超时秒数（默认 600） |
| `DEEPSEEK_BASE_URL` | API 基地址（默认 https://chat.deepseek.com） |

## 注意事项

- 日志走 stderr，stdout 只含结果（`--json` 时是纯 JSON，可直接解析）
- 错误路径：`vision` 失败（未登录）时先 `login` 再重试
- 不要用 `--stream`（流式文本）；`--json` 已禁用流式

---
name: visionary-cli
description: Analyze images with DeepSeek's vision model via the visionary-server CLI. Use this whenever the user provides an image, photo, screenshot, or document with images - run vision to look at it rather than guessing.
license: MIT
metadata:
  author: deepseek-visionary
  version: "1.0"
---

# DeepSeek Visionary CLI

用 `visionary-server` 命令行分析图片（识别图片内容、读取截图文字、理解带图文档等）。

## 核心用法

**用户提供/提到任何图片时，用下面的命令查看它**——不要自己猜图片内容，也不要让用户自己看：

```
visionary-server vision <image> [--prompt <问题>] [--thinking] [--continue] [--session-id <id>] --json
```

- `<image>`：本地路径 / base64 / data URI / `-`（stdin 管道输入）
- `--prompt`：针对图片的具体问题（省略则默认详细描述）
- `--thinking`：DeepThink 深度思考（复杂分析时开启）
- `--continue` / `--session-id`：多图对比（第一张不带，之后每张加 `--continue` 或传上一轮返回的 `session_id`）
- **必须加 `--json`**，解析返回的 JSON

成功输出（退出码 0）：

```json
{
  "text": "图片内容描述……",
  "session_id": "abc123",
  "parent_message_id": "msg456"
}
```

失败输出（退出码非零）：

```json
{ "error": "错误信息" }
```

## 示例

```bash
# 查看用户提供的截图
visionary-server vision /path/to/screenshot.png --json

# 针对图片的具体问题 + 深度思考
visionary-server vision /path/to/photo.png --json --prompt "图中有什么？" --thinking

# 管道传入图片
cat img.png | visionary-server vision - --json

# 多图对比（续聊）
visionary-server vision img1.png --json
visionary-server vision img2.png --json --continue
```

## 错误恢复

- 退出码非零且 `{"error": "未登录…"}` → 先运行 `visionary-server login`（打开浏览器自动登录，阻塞等待），再重试 `vision`
- 其他失败 → 按 `error` 字段信息处理（如图片路径不存在则换路径）

## 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 成功 |
| 1 | 失败（未登录 / 图片读取失败 / 流水线错误） |
| 2 | 参数错误 |

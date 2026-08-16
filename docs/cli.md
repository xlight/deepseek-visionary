# CLI 用法

`visionary-server` 除 `mcp-stdio`（MCP stdio serve）子命令外，提供完整的命令行子命令面，覆盖与 MCP 工具等价的核心能力。适合终端交互、脚本与 AI agent 调用（配合 `skills/visionary-cli/SKILL.md`）。

## 子命令一览

| 子命令 | 对应 MCP 工具 | 说明 |
|--------|---------------|------|
| `vision <image...>` | `deepseek_vision` | 分析一张或多张图片（完整 vision 流水线；多图一次上传联合分析） |
| `status` | `deepseek_vision_status` | 轻量鉴权状态检查 |
| `login` | `deepseek_vision_login` | 浏览器自动登录 |
| `logout` | `deepseek_vision_logout` | 清除保存的凭据 |
| `doctor` | — | 完整环境诊断（平台/浏览器/config 权限/鉴权） |
| `init [agent]` | — | 引导接入 AI agent（opencode / codex / claude / claude-desktop / cursor / dsh） |
| `mcp-stdio` | — | 显式启动 MCP stdio serve 模式 |
| （无参数） | — | 输出 help 用法（退出码 2，不进入 serve） |

## `vision` 参数

```
visionary-server vision <image>... [--prompt <q>] [--thinking] [--continue-conversation] [--session-id <id>] [--stream|--no-stream] [--json]
```

- `image`：一个或多个本地路径 / base64 / data URI；多图一次上传、fork、联合分析（与网页端多图行为一致）。`-`（从 stdin 读取全部字节）仅限单图
- `--prompt`：对图片的问题（默认：请详细描述这张图片中的内容）
- `--thinking`：启用 DeepThink 深度思考
- `--continue-conversation`：续聊，复用上一次会话（可对比多张图片）
- `--session-id`：显式复用指定会话（优先于 `--continue-conversation`）
- `--stream` / `--no-stream`：强制流式 / 强制原子输出（覆盖 TTY 检测默认）
- `--json`：原子 JSON 输出（禁用流式；与 `--stream` 互斥）

## 输出模式

`vision` 的输出模式由 **stdout 是否 TTY** 与显式开关共同决定：

| 场景 | 默认行为 | 输出形态 | 消费方 |
|------|----------|----------|--------|
| stdout 是 TTY | 文本流式 | completion 增量逐块打印 + 结尾 `[session_id: xxx]` | 终端人 |
| stdout 非 TTY | 原子文本 | 完整文本 + 结尾 session 提示 | 管道/捕获兜底 |
| `--json` | 原子 JSON | `{"text", "session_id", "parent_message_id"}` | 脚本 / agent（推荐） |

- `--stream` 强制流式、`--no-stream` 强制原子，覆盖 TTY 检测默认
- `--json` 恒为原子输出；`--json` 与 `--stream` 互斥（同时指定报错退出非零）
- `--stream` 与 `--no-stream` 互斥

## `--json` 输出形状

成功：

```json
{
  "text": "图片内容描述……",
  "session_id": "abc123",
  "parent_message_id": "msg456"
}
```

失败（未登录、图片读取失败、流水线错误）：

```json
{ "error": "未登录：请先运行 `visionary-server login` 自动登录……" }
```

## `status` 与 `--json`

```
visionary-server status
visionary-server status --json
```

默认文本输出：Authenticated / Token configured / smidV2 / Base URL / Token validation。

`--json` 输出：

```json
{
  "authenticated": true,
  "token_configured": true,
  "smid_v2": true,
  "base_url": "https://chat.deepseek.com",
  "token_valid": true
}
```

token 未配置或探针失败时：`status` 退出码非零（无论是否 `--json`）；`--json` 模式仍输出完整状态 JSON（`token_valid: false`），调用方以退出码判断可用性、以 JSON 判断细节。

## 退出码约定

| 场景 | 退出码 |
|------|--------|
| 成功 | 0 |
| 失败（未登录 / 图片读取失败 / 流水线错误 / 登录失败） | 1 |
| 参数冲突（如 `--stream --no-stream`、`--json --stream`）或用法错误 | 2（clap） |

## 示例

```bash
# 终端交互：流式输出（打字机效果）
visionary-server vision screenshot.png

# 指定问题 + DeepThink
visionary-server vision img.png --prompt "图中有什么？" --thinking

# 脚本/agent：结构化输出
visionary-server vision img.png --json

# 管道输入
cat img.png | visionary-server vision - --json

# 会话续聊
visionary-server vision img2.png --continue
visionary-server vision img3.png --session-id <上一次的 session_id>

# 登录前预检
visionary-server status --json || visionary-server login

# 强制原子输出（即使 stdout 是 TTY）
visionary-server vision img.png --no-stream
```

## 环境变量

与 MCP 模式共享：`DEEPSEEK_USER_TOKEN` / `DEEPSEEK_SMIDV2` / `DEEPSEEK_CF_CLEARANCE` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_LOGIN_TIMEOUT`。

登录页语言：默认跟随系统 locale（`LC_ALL` / `LC_MESSAGES` / `LANG`，如 `zh_CN.UTF-8` → 中文版），可用 `DEEPSEEK_LOGIN_LANG` 手动指定（如 `zh-CN` / `en` / `ja`）。

## 与 MCP 工具的关系

- 四个子命令与 MCP 工具共享同一核心实现（`pipeline.rs` / `login.rs` / `auth.rs`），无两套漂移代码
- MCP 工具行为完全不变；CLI 是同一能力的另一访问入口
- 日志统一走 stderr（serve 模式 stdout 是 MCP 协议通道，CLI 子命令 stdout 只输出结果）

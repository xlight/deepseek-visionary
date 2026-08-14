# @xlight-oss/visionary-dsh

[DeepSeek Visionary](https://github.com/xlight/deepseek-visionary) 的 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（DSH）原生插件：把 `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout` 注册为 DSH 原生工具，由 `visionary-server` CLI 支撑（DeepSeek 网页版视觉模型，**无需 API key**）。

## 特性

- **原生工具** — 结构化参数 schema 注册到 `ctx.tools`，模型直接调用，无 MCP 中间层
- **复用 Rust 管道** — 每个工具 spawn `visionary-server`（PoW → 上传 → fork → HIF → SSE 全部在 Rust 侧），插件仅做参数映射与 JSON 解析
- **宿主级权限** — 工具在 DSH 宿主进程执行（不经 bash 沙箱），会话续聊与浏览器登录不受 workspace-write 限制
- **超时有界可取消** — 每个工具声明 `timeoutMs` 并转发 `exec.signal`（abort → kill 子进程）

## 安装

```bash
# npm 包（发布后）
dsh plugin --profile web add @xlight-oss/visionary-dsh

# 本地路径（开发验证）
dsh plugin --profile web add /path/to/packages/dsh-plugin
```

`dsh plugin` 会把包安装进 profile 并通过 `dsh.bundle` 声明自动追加到 `dsh.profile.bundles` 层叠——**无需手写任何配置**。重启 DSH 后 4 个工具出现在工具目录。

验证：`dsh --profile web --dump-config` 应出现 `@xlight-oss/visionary-dsh` 层与 `visionary-vision` 插件行。

> **本地路径（开发）安装**：`dsh plugin add <path>` 以 link 方式安装，Node 从包的真实位置解析其 peer 依赖，因此本地开发需先在包目录 `pnpm install`（peer 已镜像为 devDependencies，见 `package.json`），否则加载时报 `Cannot find package '@deepseek-ai/dsh-tools'`。已发布的 npm 包无此要求（DSH 的 `profiles/node_modules` 兜底解析）。

## 前置要求

`visionary-server` 二进制需可被找到（三者任一）：

1. `Config.binaryPath`（插件配置，绝对路径）
2. `DEEPSEEK_VISIONARY_BIN` 环境变量
3. 在 PATH 中

安装二进制见 [DeepSeek Visionary 安装章节](https://github.com/xlight/deepseek-visionary#安装)（install.sh / brew / npm）。未找到时工具返回含安装指引的错误。

## 配置

在 DSH profile 的 `cordis.patch.yml`（或 `$DSH_HOME/cordis.patch.yml`）给 `visionary-vision` 行补 `config`：

```yaml
- id: visionary-vision
  config:
    binaryPath: /usr/local/bin/visionary-server
    loginTimeoutSeconds: 900        # 不设则读 DEEPSEEK_LOGIN_TIMEOUT env（默认 600）
    visionTimeoutMs: 300000
    statusTimeoutMs: 60000
```

> patch 层按 `id` 整行替换 `config`（不做键级深合并）：覆盖 `visionary-vision` 时，未写出的字段回退到下方表格中的 schema 默认值，而非保留插件包内的配置。

| 字段 | 默认 | 说明 |
|------|------|------|
| `binaryPath` | `""`（env → PATH） | 二进制绝对路径 |
| `loginTimeoutSeconds` | 600（`DEEPSEEK_LOGIN_TIMEOUT` env 优先） | 登录等待超时（秒） |
| `visionTimeoutMs` | 300000 | `deepseek_vision` 单次超时 |
| `statusTimeoutMs` | 60000 | status / logout 超时 |

## 工具

| 工具 | 说明 |
|------|------|
| `deepseek_vision` | 识图（路径 / base64 / data URI），支持 `prompt` / `thinking` / `continue_conversation` / `session_id` 多轮续聊 |
| `deepseek_vision_status` | 登录状态检查（含真实 token 探针） |
| `deepseek_vision_login` | 浏览器自动登录（阻塞，超时可配） |
| `deepseek_vision_logout` | 清除保存的凭据 |

## 与其他接入路径的关系

| 路径 | 适用 |
|------|------|
| **本插件（推荐）** | DSH 用户：原生工具、结构化 schema、宿主级权限、续聊/登录不受沙箱限制 |
| skill + CLI（`init dsh` / `skill install`） | 任何能执行 shell 的 agent：零安装配置，模型经 bash 调 `visionary-server vision <image> --json`；DSH 下续聊/登录受 bash 沙箱写限制 |
| MCP（`mcp-stdio` + 各宿主配置） | 需要标准 MCP 工具面时（Zed / OpenCode / Codex / Claude Code 等） |

三者共用同一二进制与同一份凭据（`~/.deepseek-visionary/config.json`），可并存。

## 安全提示

`deepseek_vision` 的 `image` 参数指向的文件会被**读取并上传**至 chat.deepseek.com——仅传用户有意分享的路径。

## License

MIT

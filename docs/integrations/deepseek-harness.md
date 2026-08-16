# DeepSeek Harness 接入

在 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（DSH，CLI 命令 `dsh`）中使用 DeepSeek Visionary。

DSH 提供两条接入路径，共用同一二进制与同一份凭据（`~/.deepseek-visionary/config.json`），可并存：

- **原生插件（推荐）**：`dsh plugin --profile web add @xlight-oss/visionary-dsh`——把 `deepseek_vision` 等注册为 DSH 原生工具（结构化 schema、宿主级权限，续聊/登录不受 bash 沙箱限制），并随包启用文本模型图片桥接（纯文本模型粘贴图片自动放行 + 改写为文本引导）
- **skill + CLI 轻量路线**：`visionary-server init dsh`——不配置 MCP，装好二进制 + 装好 skill 即可，模型经 shell 调 `visionary-server vision <image> --json`

## 接入方式一：原生插件（推荐）

前置：安装 `visionary-server` 二进制（[README 安装章节](../README.md#安装)，install.sh / brew / npm），并确保它能被插件找到（`Config.binaryPath` → `DEEPSEEK_VISIONARY_BIN` → PATH 任一即可）。

```bash
# npm 包（发布后）
dsh plugin --profile web add @xlight-oss/visionary-dsh

# 或本地路径（开发验证）
dsh plugin --profile web add /path/to/packages/dsh-plugin
```

`dsh plugin` 会经包内 `dsh.bundle.patch` 声明自动把 `visionary-vision` 与 `visionary-image-bridge` 两个插件行追加到 profile 组合——**无需手写任何配置**。重启 DSH 后注册 4 个原生工具，同时启用文本模型图片桥接：

| 工具 | 说明 |
|------|------|
| `deepseek_vision` | 识图（路径 / base64 / data URI），支持 `prompt` / `thinking` / `continue_conversation` / `session_id` 多轮续聊 |
| `deepseek_vision_status` | 登录状态检查 |
| `deepseek_vision_login` | 浏览器自动登录（阻塞，超时可配） |
| `deepseek_vision_logout` | 清除保存的凭据 |

> **图片桥接**：会话模型为纯文本模型（如 `deepseek-v4-flash`）时，粘贴的图片经桥接**放行 → 落盘 → 改写为文本引导**，agent 用 `deepseek_vision` 完成分析，模型只收到文本；VL 模型原生看图不受干扰。配置见插件包 [README](../../packages/dsh-plugin/README.md) 的「图片桥接」节（`settings.yaml` / 设置面板，热重载）。

原生工具在 DSH **宿主进程**执行（不经 bash 沙箱），因此 `--continue-conversation` 续聊（写 `~/.deepseek-visionary/session.json`）与 `login`（起浏览器、写 config.json）不受 `workspace-write` 写限制。安装与配置详见插件包 [README](../../packages/dsh-plugin/README.md)。

## 接入方式二：skill + CLI（轻量）

```bash
visionary-server init dsh
```

该命令会：

1. 检测 DeepSeek Harness（`dsh` 在 PATH / `$DSH_HOME` 环境变量 / `~/.dsh` 目录）
2. 将内嵌的 agent 调用契约 SKILL.md 安装到两个 DSH 技能发现根：
   - `$DSH_HOME/skills/visionary-cli/SKILL.md`（默认 `~/.dsh/skills/`，DSH user 技能根，始终扫描）
   - `~/.agents/skills/visionary-cli/SKILL.md`（DSH 默认扫描的 agents 技能根）
3. 打印使用提示

`--dry-run` 只预览两个写入路径，不落盘。此路径下 DSH 的 agent 经 bash 调 `visionary-server vision <image> --json` 识图，续聊与登录受 bash 沙箱写限制（见常见问题）。

## 手动配置（skill + CLI）

1. 安装二进制（[README 安装章节](../README.md#安装)）：`curl -LsSf ... | sh` / brew / npm，确保 `visionary-server` 在 PATH
2. 安装 skill：

   ```bash
   visionary-server skill install
   # → 写入 ~/.agents/skills/visionary-cli/SKILL.md（DSH 默认扫描此目录）
   ```

3. 重启 DSH（或等待其技能目录热加载），技能目录中即出现 **visionary-cli**

## 在 DSH 中使用

**插件路径**：模型直接调用 `deepseek_vision` 原生工具（识图 / 状态 / 登录 / 登出），无需经过 shell；工具在宿主进程执行，`--continue-conversation` 续聊与 `login` 不受 bash 沙箱写限制。

**skill + CLI 路径**：DSH 的 agent 加载 `visionary-cli` skill 后，会按契约调用（**必须加 `--json` 原子输出**）：

```bash
visionary-server vision /path/to/image.png --json --prompt "图中有什么？" --thinking
```

成功输出（退出码 0）：

```json
{ "text": "图片内容描述……", "session_id": "abc123", "parent_message_id": "msg456" }
```

首次使用前先登录：

```bash
visionary-server login        # 浏览器自动登录（仅首次）
visionary-server status --json  # 预检登录状态
```

## 技能发现根说明

DSH 的技能文件系统按以下根顺序发现技能（一级目录 `<root>/<name>/SKILL.md` 或 `<root>/<name>.md`）：

| 根 | 路径 |
|----|------|
| 项目级 | `<项目根>/.dsh/skills`、`<项目根>/.agents/skills` |
| 用户级 | `$DSH_HOME/skills`（默认 `~/.dsh/skills`） |
| 用户级 | `$DSH_AGENTS_HOME`/`~/.agents/skills` |

`init dsh` 覆盖两个用户级根，因此无论 DSH 的 `agentsHome` 是否被修改过，`~/.dsh/skills` 都能兜底发现。

## 验证

```bash
# 插件路径：
dsh --profile web --dump-config   # 应出现单个 @xlight-oss/visionary-dsh 层，含 visionary-vision 与 visionary-image-bridge 两个插件行
# 重启 DSH 后问 agent "你能看图吗"，或直接手测工具调用

# skill + CLI 路径：
# 1) 二进制与 skill 就位
visionary-server status --json

# 2) 重启 DSH 后，技能目录应出现 visionary-cli（或问 agent "你能看图吗"）
# 3) 直接手测
visionary-server vision screenshot.png --json
```

## 常见问题

- **skill 没出现在技能目录**：确认 DSH 已重启（技能根有 Chokidar 观察，新增文件一般会被自动发现）；确认装到了被扫描的根（`init dsh` 会同时写两个用户级根）。
- **未登录**：`visionary-server login` 自动登录，或设置环境变量 `DEEPSEEK_USER_TOKEN` 注入 token。`login` 会写 config.json 并打开浏览器，**建议在用户自己的终端执行**（而非在 DSH 会话内），避免 DSH 沙箱的写限制。
- **单次识图正常但 `--continue-conversation` 续聊不生效**：DSH 的 bash 沙箱（默认 `workspace-write`）只允许写 workspace 与 /tmp；`visionary-server` 把会话状态持久化在 `~/.deepseek-visionary/session.json`（工作区外），该写入会被沙箱拒绝，会话不跨调用持久（单次识图不受影响——读图与网络请求不受文件沙箱限制）。需要多图对比时，在 DSH 会话中使用 `danger-full-access` 沙箱模式，或接受单次调用。
- **`visionary-server: command not found`**：二进制不在 DSH 进程的 PATH，用绝对路径调用，或重新安装并确认 PATH。
- **`$DSH_HOME` 自定义**：`init dsh` 遵循 `$DSH_HOME` 环境变量（未设置时回退 `~/.dsh`）；若 `$DSH_AGENTS_HOME` 也被自定义，`~/.dsh/skills` 根仍会被 DSH 扫描，不受影响。
- **安全提示：`image` 指向的文件会被读取并上传**：`vision`（以及插件工具 `deepseek_vision`）的 `image` 参数指向的本地文件会被程序读取并上传至 chat.deepseek.com 供视觉模型分析——**仅传有意分享的路径**。模型或提示注入可能诱导其读取本地文件（如 `~/.ssh/id_rsa`、`.env`）借上传通道外传；不要把敏感路径交给模型自由选择，agent 会话中提供图片时同样遵循此约束。

## 进阶：MCP 模式（可选）

DSH 也支持 MCP（`dsh-mcp-client` 插件），想以 MCP 工具形式暴露 `deepseek_vision` 时，可手动在 DSH profile 的 `cordis.patch.yml`（或 `$DSH_HOME/cordis.patch.yml`）追加：

```yaml
- insert:
    - id: mcp-deepseek-visionary
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: deepseek-visionary
        transport: stdio
        command: visionary-server
        args: ['mcp-stdio']
```

重启 DSH 后模型即可调用 `mcp__deepseek-visionary__deepseek_vision`。原生插件与 skill + CLI 路线均无需 MCP 配置；MCP 仅作为需要标准工具面时的可选通道。

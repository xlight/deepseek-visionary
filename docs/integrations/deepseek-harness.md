# DeepSeek Harness 接入

在 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（DSH，CLI 命令 `dsh`）中使用 DeepSeek Visionary。

DSH 采用 **skill + CLI 轻量路线**：不配置 MCP，装好二进制 + 装好 skill 即可。DSH 中的 agent 能执行 shell 命令，并默认扫描 `~/.agents/skills` 与 `~/.dsh/skills` 作为技能根——正好是 `visionary-server skill install` 的安装位置。

## 一键接入（推荐）

```bash
visionary-server init dsh
```

该命令会：

1. 检测 DeepSeek Harness（`dsh` 在 PATH / `$DSH_HOME` 环境变量 / `~/.dsh` 目录）
2. 将内嵌的 agent 调用契约 SKILL.md 安装到两个 DSH 技能发现根：
   - `$DSH_HOME/skills/visionary-cli/SKILL.md`（默认 `~/.dsh/skills/`，DSH user 技能根，始终扫描）
   - `~/.agents/skills/visionary-cli/SKILL.md`（DSH 默认扫描的 agents 技能根）
3. 打印使用提示

`--dry-run` 只预览两个写入路径，不落盘。

## 手动配置

1. 安装二进制（[README 安装章节](../README.md#安装)）：`curl -LsSf ... | sh` / brew / npm，确保 `visionary-server` 在 PATH
2. 安装 skill：

   ```bash
   visionary-server skill install
   # → 写入 ~/.agents/skills/visionary-cli/SKILL.md（DSH 默认扫描此目录）
   ```

3. 重启 DSH（或等待其技能目录热加载），技能目录中即出现 **visionary-cli**

## 在 DSH 中使用

DSH 的 agent 加载 `visionary-cli` skill 后，会按契约调用（**必须加 `--json` 原子输出**）：

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
# 1) 二进制与 skill 就位
visionary-server status --json

# 2) 重启 DSH 后，技能目录应出现 visionary-cli（或问 agent "你能看图吗"）
# 3) 直接手测
visionary-server vision screenshot.png --json
```

## 常见问题

- **skill 没出现在技能目录**：确认 DSH 已重启（技能根有 Chokidar 观察，新增文件一般会被自动发现）；确认装到了被扫描的根（`init dsh` 会同时写两个用户级根）。
- **未登录**：`visionary-server login` 自动登录，或设置环境变量 `DEEPSEEK_USER_TOKEN` 注入 token。`login` 会写 config.json 并打开浏览器，**建议在用户自己的终端执行**（而非在 DSH 会话内），避免 DSH 沙箱的写限制。
- **单次识图正常但 `--continue` 续聊不生效**：DSH 的 bash 沙箱（默认 `workspace-write`）只允许写 workspace 与 /tmp；`visionary-server` 把会话状态持久化在 `~/.deepseek-visionary/session.json`（工作区外），该写入会被沙箱拒绝，会话不跨调用持久（单次识图不受影响——读图与网络请求不受文件沙箱限制）。需要多图对比时，在 DSH 会话中使用 `danger-full-access` 沙箱模式，或接受单次调用。
- **`visionary-server: command not found`**：二进制不在 DSH 进程的 PATH，用绝对路径调用，或重新安装并确认 PATH。
- **`$DSH_HOME` 自定义**：`init dsh` 遵循 `$DSH_HOME` 环境变量（未设置时回退 `~/.dsh`）；若 `$DSH_AGENTS_HOME` 也被自定义，`~/.dsh/skills` 根仍会被 DSH 扫描，不受影响。

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

重启 DSH 后模型即可调用 `mcp__deepseek-visionary__deepseek_vision`。skill + CLI 路线无需任何配置，是推荐方式。

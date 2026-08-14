# agent-onboarding Specification

## Purpose
定义 `visionary-server init` 子命令：检测本机已安装的 AI agent（opencode、Codex、Claude Code、Cursor、Claude Desktop），按各 agent 的 MCP 配置形状写入配置，支持写前备份、dry-run 预览与多 agent 批量/免交互接入，并提供接入文档矩阵。

## Requirements

### Requirement: init 子命令检测已安装 agent
`visionary-server init` SHALL 检测本机已安装的 AI agent，支持至少：opencode、Codex（`codex` CLI）、Claude Code（`claude` CLI）、Cursor、Claude Desktop、DeepSeek Harness（`dsh`）。检测依据为可执行文件是否在 PATH 中（Windows 上同时检查 `where`）或配置目录是否存在。`init` 无参数时 SHALL 以交互/列表方式展示检测结果；传入 agent 名参数时 SHALL 仅处理指定 agent。

#### Scenario: 检测到多个 agent
- **WHEN** 本机同时安装了 opencode 与 Codex CLI，用户执行 `visionary-server init`
- **THEN** 输出列出已检测到的 agent 及各自配置文件路径，等待用户选择或直接给出各 agent 的配置建议

#### Scenario: 未检测到任何 agent
- **WHEN** 本机未安装任何受支持的 agent
- **THEN** 输出提示信息并指向 `docs/integrations/` 文档中的手动配置路径，退出码非零

#### Scenario: 指定 agent 但未安装
- **WHEN** 用户执行 `visionary-server init codex` 但本机无 codex CLI
- **THEN** 输出"未检测到 Codex"提示并给出安装与手动配置指引

#### Scenario: visionary-server 不在 PATH
- **WHEN** 用户执行 `init` 时 `visionary-server` 不在 PATH（如手动下载裸二进制未加入 PATH）
- **THEN** init 提示二进制未找到，给出安装方式（install.sh / brew / npm）或指引使用绝对路径，不写入无效配置

### Requirement: 按 agent 配置形状写入 MCP 配置
`visionary-server init <agent>` SHALL 按各 agent 的 MCP 配置格式写入对应配置，注册名为 `deepseek-visionary`，启动命令指向 `visionary-server mcp-stdio`（显式 MCP stdio 模式）。各 agent 形状 MUST 符合以下要求：

- **opencode**：写顶层 `mcp` 键，`type: "local"`，`command` 为数组 `["visionary-server", "mcp-stdio"]`，并显式设置 `timeout: 60000`（规避 npx 冷启动与首次启动超时）
- **Codex**：优先 `codex mcp add deepseek-visionary -- visionary-server mcp-stdio`（`--` 分隔符形式）；失败时写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]`（含 `command` 与 `args = ["mcp-stdio"]`）
- **Claude Code**：优先调用 `claude mcp add --transport stdio deepseek-visionary -- visionary-server mcp-stdio`；失败时写入用户级 MCP 配置的 `mcpServers` 形状（`args: ["mcp-stdio"]`）
- **Cursor**：写入 `mcpServers` 形状到 Cursor 的 MCP 配置（用户级或项目级 `.cursor/mcp.json`），`args: ["mcp-stdio"]`
- **Claude Desktop**：写入 `mcpServers` 形状到 Claude Desktop 的 `claude_desktop_config.json`，`args: ["mcp-stdio"]`

#### Scenario: 配置 opencode
- **WHEN** 用户执行 `visionary-server init opencode` 且 opencode 已安装
- **THEN** opencode 配置文件的顶层 `mcp` 键下写入 `deepseek-visionary` 条目，含 `type: "local"`、`command: ["visionary-server", "mcp-stdio"]`、`timeout: 60000`

#### Scenario: 配置 Codex
- **WHEN** 用户执行 `visionary-server init codex`
- **THEN** 优先调用 `codex mcp add deepseek-visionary -- visionary-server mcp-stdio` 注册；CLI 不可用时写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]` 段（`args = ["mcp-stdio"]`），注册 stdio 本地服务

#### Scenario: 配置 Claude Code
- **WHEN** 用户执行 `visionary-server init claude` 且 `claude` CLI 可用
- **THEN** 调用 `claude mcp add` 注册服务（命令带 `mcp-stdio` 参数）；若 CLI 不可用则直接写入用户级配置文件的 `mcpServers` 形状（`args: ["mcp-stdio"]`）

#### Scenario: 配置 Cursor
- **WHEN** 用户执行 `visionary-server init cursor`
- **THEN** 在 Cursor 的 MCP 配置中写入 `mcpServers` 下的 `deepseek-visionary` 条目（`command` + `args: ["mcp-stdio"]` 形式）

#### Scenario: 配置 Claude Desktop
- **WHEN** 用户执行 `visionary-server init claude-desktop`
- **THEN** 在 `claude_desktop_config.json` 的 `mcpServers` 下写入 `deepseek-visionary` 条目（`args: ["mcp-stdio"]`）

### Requirement: 写入前备份
`visionary-server init` SHALL 在修改任何已存在的配置文件前，先创建该文件的备份副本（追加时间戳后缀，如 `config.json.bak.20260809T120000`），备份失败时 MUST 中止写入并报错，绝不静默覆盖用户配置。

#### Scenario: 已有配置时备份
- **WHEN** 目标配置文件已存在且包含其他 MCP 服务配置，用户执行 `init`
- **THEN** 先创建带时间戳的备份文件，再合并写入 `deepseek-visionary` 条目，原有条目不被删除

#### Scenario: 备份失败中止
- **WHEN** 目标配置文件存在但无法创建备份（如权限不足）
- **THEN** 不修改原文件，输出错误并退出非零

### Requirement: dry-run 预览
`visionary-server init` SHALL 支持 `--dry-run` 参数：输出将要写入的配置片段与目标文件路径，但不实际创建或修改任何文件。

#### Scenario: 预览配置
- **WHEN** 用户执行 `visionary-server init opencode --dry-run`
- **THEN** stdout 展示将写入 opencode 配置的 JSON 片段与目标路径，磁盘上的配置文件保持不变

### Requirement: init 支持多 agent 批量与免交互
`visionary-server init` SHALL 支持社区主流的多 agent 选择方式：除位置参数 `init <agent>` 外，支持 `--opencode` / `--codex` / `--claude` / `--cursor` / `--claude-desktop` 多选 flags 一次性写入多个 agent 配置；支持 `--yes` 跳过确认/交互直接写入；支持 `--dry-run` 预览（与 `--yes` 可组合）。位置参数与多选 flags 同时传入时 SHALL 报错提示二选一。

#### Scenario: 批量配置多个 agent
- **WHEN** 用户执行 `visionary-server init --opencode --codex --yes`
- **THEN** 依次为 opencode 与 Codex 写入配置，不逐一确认，结束后汇总列出各 agent 写入结果

#### Scenario: 多选 flags 与位置参数冲突
- **WHEN** 用户同时传入位置参数与多选 flags（如 `init opencode --codex`）
- **THEN** 输出参数冲突错误并提示二选一，退出非零

### Requirement: 接入文档矩阵
仓库 SHALL 提供 `docs/integrations/` 目录，为每个受支持 agent（opencode、Codex、Claude Code、Claude Desktop、Cursor、Zed、DeepSeek Harness）提供一份 Markdown 接入文档。每份文档 MUST 包含：接入方式、手动配置示例、常见问题（如首次启动超时、登录与环境变量）。所有 MCP 配置示例 SHALL 使用 `visionary-server mcp-stdio` 启动命令。opencode / Codex / Claude Code / Claude Desktop / Cursor 文档以 `visionary-server init <agent>` 作为一键接入方式；Zed 文档 SHALL 以扩展市场安装为接入方式（Zed 无 MCP 配置文件，不在 init 支持范围）；DeepSeek Harness 文档以 `visionary-server init dsh` 作为一键接入方式，主推 skill + CLI 轻量路线（不写 MCP 配置）。README 的安装章节 SHALL 以 **CLI + skill 为优先推荐方式**（`visionary-server skill install` + 直接调用 `vision` 子命令），MCP 接入（init / Zed 扩展）作为进阶选项。README 或文档 SHALL 提及新兴安装通道 `apm install --mcp io.github.xlight/deepseek-visionary`（Microsoft APM，复用 MCP Registry 标识，无需额外建设）。

#### Scenario: 文档覆盖全部受支持 agent
- **WHEN** 检查 `docs/integrations/` 目录
- **THEN** 存在 opencode.md、codex.md、claude-code.md、claude-desktop.md、cursor.md、zed.md、deepseek-harness.md 七份文档，每份均含接入方式与手动配置两种路径，且所有 MCP 配置示例使用 `mcp-stdio` 参数；deepseek-harness.md 以 `init dsh` 为一键接入方式并说明 skill 安装位置

#### Scenario: README 指引
- **WHEN** 用户阅读 README 安装章节
- **THEN** 首先看到"CLI + skill"优先使用方式（`visionary-server skill install` + `vision` 子命令），MCP 接入（`init` / Zed 扩展）作为进阶选项，并链接到 `docs/integrations/`

### Requirement: init 检测 DeepSeek Harness 并经 skill 接入
`visionary-server init dsh` SHALL 将 DeepSeek Harness（DSH）作为受支持 agent 接入。接入方式为 **skill + CLI 轻量路线**：不写任何 MCP 配置，而是将内嵌的 agent 调用契约 SKILL.md 安装到 DSH 的技能发现根，使 DSH 中的 agent 可直接经 shell 调用 `visionary-server vision <image> --json` 识图。

DSH 检测 SHALL 依据以下任一条件：`dsh` 可执行文件在 PATH 中；`$DSH_HOME` 环境变量已设置（其 `profiles` 目录存在）；用户主目录下 `~/.dsh/profiles` 目录存在（DSH 初始化过 `dsh web` / `dsh --profile headless` 即会创建）。DSH 根目录解析 SHALL 优先 `$DSH_HOME` 环境变量，未设置时回退 `~/.dsh`。

`init dsh` SHALL 将内嵌 SKILL.md 安装到以下两个 DSH 技能发现根（自动创建目录，已存在则覆盖）：
- `$DSH_HOME/skills/visionary-cli/SKILL.md`（DSH user 技能根）
- `~/.agents/skills/visionary-cli/SKILL.md`（DSH 默认扫描的 agents 技能根）

`init dsh` SHALL 支持 `--dry-run`（预览将写入的路径与内容，不落盘）与 `--yes`。DSH 未检测到时 SHALL 仍可显式执行 `init dsh`（安装 skill 到两技能根），但在交互式检测列表中标记为未安装。

#### Scenario: init dsh 安装 skill 到两个技能根
- **WHEN** 用户执行 `visionary-server init dsh`
- **THEN** 内嵌 SKILL.md 被写入 `$DSH_HOME/skills/visionary-cli/SKILL.md` 与 `~/.agents/skills/visionary-cli/SKILL.md`（目录自动创建），两文件内容与内嵌一致，stdout 汇总列出写入路径，退出码为 0

#### Scenario: DSH 根目录解析优先级
- **WHEN** 环境变量 `$DSH_HOME` 已设置且用户主目录同时存在 `~/.dsh`
- **THEN** `init dsh` 将 skill 安装到 `$DSH_HOME/skills/visionary-cli/SKILL.md`（而非 `~/.dsh/skills/...`）

#### Scenario: init dsh --dry-run 不落盘
- **WHEN** 用户执行 `visionary-server init dsh --dry-run`
- **THEN** stdout 预览将写入的两个 skill 路径，磁盘上不创建任何文件，退出码为 0

#### Scenario: 检测列表包含 DSH
- **WHEN** 本机装有 DSH（`dsh` 在 PATH 或 `~/.dsh/profiles` 存在），用户执行无参数 `visionary-server init`
- **THEN** 输出列表包含 DSH 条目及其根目录路径，标记为已检测

### Requirement: DSH 接入文档说明沙箱与持久化约束
DeepSeek Harness 接入文档（`docs/integrations/deepseek-harness.md`）SHALL 说明 DSH bash 沙箱对 `visionary-server` 的影响：

- `vision` 的单次识图 SHALL 说明不受 DSH 沙箱（`workspace-write` 模式）影响：读取图片路径与网络请求均不受文件沙箱限制
- `--continue` 会话续聊 SHALL 说明其持久化写入 `~/.deepseek-visionary/session.json`（工作区外），在 DSH 默认 `workspace-write` 沙箱下该写入会被拒绝、会话状态不跨调用持久；需要多图对比时 SHALL 建议 DSH 会话使用 `danger-full-access` 沙箱模式或接受单次调用
- `login` SHALL 说明应在用户终端执行（写入 config.json 并启动浏览器），或经 `DEEPSEEK_USER_TOKEN` 环境变量注入 token，而非在 DSH 会话内执行
- 安全提示 SHALL 说明：`vision` 的 `image` 参数指向的文件会被读取并上传至 DeepSeek 服务，仅传有意分享的路径（模型或提示注入可能借其外传本地文件）

#### Scenario: 文档覆盖沙箱约束
- **WHEN** 用户阅读 `docs/integrations/deepseek-harness.md` 的常见问题章节
- **THEN** 能看到单次识图不受沙箱限制、`--continue` 续聊需 `danger-full-access` 或接受单次、`login` 建议在终端执行、`image` 路径会被读取并上传（仅传有意分享的路径）的说明

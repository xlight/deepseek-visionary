# mcp-stdio-subcommand Design

## Context

`visionary-server` 目前无参数时直接进入 MCP stdio serve 模式（历史约束：所有 agent 配置以 `command: ["visionary-server"]` 无参启动，见已归档 generalize-mcp-distribution 的 design 决策 1）。项目定位转向"CLI 优先"：CLI + skill 是主推使用方式，MCP 是可选通道。无参数在终端直接运行会挂起等待 stdin，对用户不友好。

本次改动将无参数行为反转为"输出 help"，MCP stdio 服务改为显式 `mcp-stdio` 子命令，并同步更新所有 agent 接入配置形状与文档。

## Goals / Non-Goals

**Goals:**
- 无参数运行输出 help 并退出（exit 2），符合 CLI 工具惯例
- 新增 `mcp-stdio` 子命令显式启动 MCP stdio 服务，行为与原无参数路径完全一致
- 所有 agent 接入通道（init / Zed 扩展壳 / mcpb manifest / CI smoke 脚本 / 手动配置文档）统一为 `visionary-server mcp-stdio`
- README 安装章节改为"CLI + skill 优先"

**Non-Goals:**
- 不支持除 stdio 外的其他 MCP 传输（http / websocket 未来再扩展，`mcp-stdio` 命名为此预留）
- 不处理 Zed 扩展市场 PR #7159 的上架状态
- 不做存量配置的自动迁移（硬切，靠 help 提示引导）

## Decisions

### 决策 1：命令名用 `mcp-stdio`（而非 serve / mcp）

- **选择**：`mcp-stdio`，无参数。
- **备选**：`serve`（通用但未指明 MCP 传输）；`mcp`（未来 http/websocket 传输时名不副实）。
- **理由**：`mcp-stdio` 精确描述"MCP stdio 传输"语义；未来若支持 http 传输可加 `mcp-http` 平行命令，命名空间一致。

### 决策 2：无参数行为用 clap `arg_required_else_help`（exit 2）

- **选择**：`#[command(arg_required_else_help = true)]`，无参数时 clap 自动输出 help 到 stderr 并 exit 2。
- **备选**：手动 `None` 分支 `print_help()` + exit 0（docker/cargo 风格）；exit 1（git 风格）。
- **理由**：
  - 无参数对 CLI 工具是"缺参数"，exit 2 符合 clap"参数错误"惯例
  - 若存量 MCP 宿主仍以旧配置无参启动，进程立即 exit 2 + stderr help，宿主能明确看到"启动失败 + 原因"，而非静默挂起或协议失败
  - 实现最简：一行属性，help 排版由 clap 自动处理
- **注意**：`arg_required_else_help` 只作用于**顶层**无参数，不影响 `skill`（无 action 默认 install）与 `--version`（clap 自动处理）

### 决策 3：`mcp-stdio` 内部复用现有 `serve()` 函数

- **选择**：`mcp-stdio` 子命令直接调用现有 `async fn serve()`，零逻辑改动；原 `None` 分支删除。
- **理由**：serve 路径已有完整测试（MCP 握手、4 工具暴露），不重复实现。

### 决策 4：所有 agent 接入形状统一加 `mcp-stdio` 参数

- **opencode**：`command: ["visionary-server"]` → `["visionary-server", "mcp-stdio"]`
- **Codex CLI**：`codex mcp add deepseek-visionary -- visionary-server` → `-- visionary-server mcp-stdio`
- **Claude CLI**：`claude mcp add ... -- visionary-server` → `-- visionary-server mcp-stdio`
- **JSON 形状**（claude / claude-desktop / cursor）：`command: "visionary-server", args: []` → `args: ["mcp-stdio"]`
- **TOML 形状**（codex 兜底）：`args = []` → `args = ["mcp-stdio"]`
- **Zed 扩展壳**：`Command { args: vec![] }` → `args: vec!["mcp-stdio"]`
- **mcpb manifest**：`"args": []` → `"args": ["mcp-stdio"]`
- **mcp_probe.py**：`Popen([binary])` → `Popen([binary, "mcp-stdio"])`

### 决策 5：硬切 + help 提示迁移

- **选择**：直接 breaking，help 输出中提示"MCP 模式请用 `mcp-stdio`；旧配置请重新运行 `visionary-server init`"。
- **备选**：stdin 探测智能兼容（首字节 `{` 则 serve）—— hacky、有竞态、违背"CLI 纯净"目标，否决。
- **理由**：项目处于 0.x 早期，存量用户少，正是 breaking change 时机；`init` 一键迁移成本低。

## Risks / Trade-offs

- [存量 agent 配置升级后失效] → help 明确提示迁移路径；`init` 一键重配；README 标注 breaking change
- [Zed 扩展壳若已上架则旧版本无参启动失效] → PR #7159 暂不处理；新扩展版本（若发布）使用 `mcp-stdio` args
- [arg_required_else_help 影响其他无参子命令] → 已确认只作用于顶层；`skill` / `vision` 缺参行为不受影响（子命令自身参数仍按原逻辑报错）

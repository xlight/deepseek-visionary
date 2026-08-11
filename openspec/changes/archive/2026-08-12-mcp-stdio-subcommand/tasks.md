# mcp-stdio-subcommand Tasks

## 1. CLI 核心（cli.rs）

- [x] 1.1 在 `Command` 枚举新增 `McpStdio` 变体（`/// 启动 MCP stdio 服务（MCP 模式入口）。`），`run()` 匹配分支改为 `Some(Command::McpStdio) => serve().await`
- [x] 1.2 `Cli` 结构体加 `#[command(arg_required_else_help = true)]`，删除 `None => serve().await` 分支（无参数由 clap 输出 help 并 exit 2）
- [x] 1.3 更新 `cli.rs` 顶部文档注释（"无参数时进入 MCP stdio serve 模式" → "无参数输出 help；`mcp-stdio` 子命令进入 serve"）
- [x] 1.4 更新 `cli.rs` 单元测试：`no_args_means_serve` 改为 `no_args_means_help`（断言无参解析失败或 command 为 None 且 clap 报 help）；新增 `mcp_stdio_subcommand_parses`（断言 `McpStdio` 可解析）

## 2. agent 接入形状（onboarding.rs）

- [x] 2.1 opencode：`command: ["visionary-server"]` → `["visionary-server", "mcp-stdio"]`
- [x] 2.2 codex CLI：`.args(["mcp", "add", SERVER_NAME, "--", "visionary-server"])` → 追加 `"mcp-stdio"`
- [x] 2.3 claude CLI：`.args([... "visionary-server"])` → 追加 `"mcp-stdio"`
- [x] 2.4 JSON 形状（claude / claude_desktop / cursor）：`entry = json!({ "command": "visionary-server", "args": [] })` → `args: ["mcp-stdio"]`
- [x] 2.5 codex_section_toml：`args` 数组改为 `["mcp-stdio"]`
- [x] 2.6 更新 onboarding.rs 单元测试断言（`command = "visionary-server"` 相关断言同步加 mcp-stdio）

## 3. Zed 扩展壳（visionary-zed-ext）

- [x] 3.1 `context_server_command` 返回 `Command { args: vec![] }` → `args: vec!["mcp-stdio".to_string()]`

## 4. 脚本同步

- [x] 4.1 `scripts/mcp_probe.py`：`spawn()` 的 `Popen([binary])` → `Popen([binary, "mcp-stdio"])`
- [x] 4.2 `scripts/build_mcpb.py`：manifest `mcp_config.args` → `["mcp-stdio"]`

## 5. 集成测试（tests/cli.rs）

- [x] 5.1 `no_args_starts_mcp_serve_and_handshakes` 改为 `mcp_stdio_starts_mcp_serve_and_handshakes`：`Command::new(bin()).arg("mcp-stdio")` 启动握手
- [x] 5.2 新增 `no_args_prints_help_and_exits_2`：无参数运行断言退出码 2 且 stderr 含 usage/help

## 6. 文档

- [x] 6.1 `README.md`：CLI 工具表"（无参数）进入 MCP stdio serve"改为"（无参数）输出 help；`mcp-stdio` 进入 MCP serve"；架构图 MCP 入口更新；安装章节改为 **CLI + skill 优先**，MCP 接入降级为进阶选项
- [x] 6.2 `docs/cli.md`：子命令一览表"（无参数）→ 进入 MCP stdio serve 模式"更新为"（无参数）→ 输出 help；`mcp-stdio` → MCP stdio serve"
- [x] 6.3 `docs/integrations/opencode.md`：`command: ["visionary-server"]` 两处 → `["visionary-server", "mcp-stdio"]`
- [x] 6.4 `docs/integrations/codex.md`：`command = "visionary-server", args = []` → `args = ["mcp-stdio"]`
- [x] 6.5 `docs/integrations/claude-code.md` / `claude-desktop.md` / `cursor.md`：`args: []` → `args: ["mcp-stdio"]`

## 7. 验证

- [x] 7.1 `cargo build -p visionary-server` 编译通过
- [x] 7.2 `cargo test -p visionary-server` 全绿（单元 + 集成）
- [x] 7.3 `cargo clippy -p visionary-server --all-targets` 无警告
- [x] 7.4 冒烟验证：无参数运行输出 help 且退出码 2；`mcp-stdio` 能完成 MCP 握手；`init --dry-run` 预览的配置含 `mcp-stdio` 参数

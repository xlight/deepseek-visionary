## MODIFIED Requirements

### Requirement: mcp-stdio 子命令启动 MCP stdio 服务
`visionary-server` SHALL 提供 `mcp-stdio` 子命令，显式启动 MCP stdio 服务：初始化日志（stderr）、加载配置、启动 MCP stdio 服务并阻塞等待连接结束。该路径 MUST 与原无参数 serve 行为完全一致，不改变协议行为、配置加载或工具暴露。

#### Scenario: mcp-stdio 启动
- **WHEN** 用户执行 `visionary-server mcp-stdio`
- **THEN** 服务正常启动 MCP stdio serve，暴露 `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout` 四个工具

#### Scenario: 未知子命令
- **WHEN** 用户传入不存在的子命令（如 `visionary-server frobnicate`）
- **THEN** 程序在 stderr 输出错误与用法提示，并以非零退出码退出，不进入 serve 模式

## ADDED Requirements

### Requirement: 无参数输出 help
`visionary-server` SHALL 在无任何命令行参数时输出 help 用法信息（含全部子命令列表与 MCP 模式提示），并以退出码 2 退出，MUST 不进入 MCP stdio serve 模式。help 输出 SHALL 提示 MCP 模式使用 `mcp-stdio` 子命令，并提示存量配置可通过重新运行 `visionary-server init` 迁移。

#### Scenario: 无参数输出 help
- **WHEN** 用户直接执行 `visionary-server`（无参数）
- **THEN** stderr 输出 help 用法信息（含 `mcp-stdio` 子命令说明），退出码为 2，不启动 MCP serve

#### Scenario: 无参数不进入 serve
- **WHEN** 用户直接执行 `visionary-server`（无参数）且 stdin 有数据
- **THEN** 程序输出 help 并退出，不将 stdin 当作 MCP 协议通道处理

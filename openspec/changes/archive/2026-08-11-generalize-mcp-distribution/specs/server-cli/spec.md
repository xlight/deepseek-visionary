## ADDED Requirements

### Requirement: 无参数启动进入 MCP serve 模式
`visionary-server` SHALL 在无任何命令行参数时保持现有行为：初始化日志（stderr）、加载配置、启动 MCP stdio 服务并阻塞等待连接结束。该路径 MUST 与引入 CLI 前完全兼容，不改变协议行为、配置加载或工具暴露。

#### Scenario: 无参数启动
- **WHEN** 用户直接执行 `visionary-server`（无参数）
- **THEN** 服务正常启动 MCP stdio serve，暴露 `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout` 四个工具

#### Scenario: 未知子命令
- **WHEN** 用户传入不存在的子命令（如 `visionary-server frobnicate`）
- **THEN** 程序在 stderr 输出错误与用法提示，并以非零退出码退出，不进入 serve 模式

### Requirement: 版本输出
`visionary-server` SHALL 支持 `--version` / `-V` 参数，在 stdout 输出形如 `visionary-server <semver>` 的版本号（取自 crate 版本），随后退出。

#### Scenario: 查询版本
- **WHEN** 用户执行 `visionary-server --version`
- **THEN** stdout 输出包含当前 crate 版本号的单行文本，退出码为 0

### Requirement: doctor 诊断
`visionary-server` SHALL 提供 `doctor` 子命令，输出环境与配置诊断信息，包括：配置文件路径及权限、浏览器（Chrome 系）可执行文件检测结果、保存的凭据状态（复用现有 token 校验探针）、平台与架构信息。诊断结果 SHALL 以可读文本输出到 stdout，检查失败项 MUST 明确标注，最终以非零退出码退出当存在严重失败项（如无可用浏览器且无凭据）。

#### Scenario: 正常环境诊断
- **WHEN** 用户执行 `visionary-server doctor` 且环境正常（有浏览器、有有效 token）
- **THEN** stdout 逐项输出各检查项并标注 OK，退出码为 0

#### Scenario: 缺少浏览器
- **WHEN** 系统未检测到任何 Chrome 系浏览器
- **THEN** 浏览器检查项标注为失败，输出手动配置指引，退出码为非零

#### Scenario: token 无效
- **WHEN** 保存的 token 未通过校验探针
- **THEN** 凭据检查项标注为无效，提示重新登录或检查 `DEEPSEEK_USER_TOKEN` 环境变量，退出码为非零

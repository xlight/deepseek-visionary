## ADDED Requirements

### Requirement: 扩展清单与启动命令
Zed 扩展 SHALL 在 `extension.toml` 中声明 `[context_servers.deepseek-visionary]`（段内含 `name` 字段），实现 `context_server_command` 返回启动 MCP 服务的命令、参数与环境变量，并实现 `context_server_configuration` 提供安装引导与可选设置（不含必填配置项）。

#### Scenario: Zed 请求启动上下文服务器
- **WHEN** Zed 需要启动该 context server
- **THEN** 扩展返回指向 `visionary-server` 二进制的有效 Command

### Requirement: 二进制获取与缓存
扩展 SHALL 按优先级解析服务器命令：用户设置提供的 `server_path` → 本地已安装或缓存的 `visionary-server` 二进制 → 从 GitHub Releases 下载与当前平台匹配的版本。下载发生在 `context_server_command` 首次被调用时，按版本目录缓存供后续复用，仅当有新版本时更新。

#### Scenario: 已缓存二进制
- **WHEN** 本地缓存存在对应平台的二进制
- **THEN** 直接使用缓存，不发起下载

#### Scenario: 用户提供 server_path
- **WHEN** 用户在设置中指定 `server_path` 且该路径存在
- **THEN** 直接使用该路径启动，不触发下载

#### Scenario: 首次下载
- **WHEN** 本地无二进制且网络可用
- **THEN** 下载并解压对应平台 release 资产、设置可执行权限后启动

#### Scenario: 下载失败
- **WHEN** 下载或解压失败
- **THEN** 返回明确错误信息，指导用户手动安装

### Requirement: 环境变量透传
扩展 SHALL 从**扩展进程环境**读取 `DEEPSEEK_USER_TOKEN`（若存在）并透传给子进程；该变量仅为覆盖，凭据存储于 `config.json` 时由服务进程自行读取；不越权读取项目目录之外的本地文件。

> 修订说明：zed_extension_api 0.7 的 `context_server_command` 只能拿到 `Project`（仅 `worktree_ids()`，无构造 `Worktree` 的 API），无法读取 `worktree.shell_env()`，因此原“按 worktree 环境透传”降级为“扩展进程环境透传”（与 design.md 决策 1 修订一致）。用户如在启动 Zed 的 shell 中设置 `DEEPSEEK_USER_TOKEN`，子进程会自然继承。

#### Scenario: 环境变量透传
- **WHEN** 扩展进程环境中存在 `DEEPSEEK_USER_TOKEN`
- **THEN** 该变量被包含在启动命令的 env 中

### Requirement: 平台支持
扩展 SHALL 支持 macOS（arm64/x86_64）、Linux（x86_64/aarch64）与 Windows（x86_64），并为各平台解析正确的 release 资产。

#### Scenario: 各平台解析
- **WHEN** 扩展在任一受支持平台上运行
- **THEN** 解析到与该平台匹配的二进制资产

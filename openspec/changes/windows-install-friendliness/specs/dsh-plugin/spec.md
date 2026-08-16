# dsh-plugin (DSH 插件)

## ADDED Requirements

### Requirement: Windows npm 全局安装场景下自动定位二进制真身

`npm install -g @xlight-oss/visionary-server` 在 Windows 的 PATH 中只生成 `.cmd` / `.ps1` shim（node 包装），真实 exe 位于包内 `node_modules/.bin_real/`。插件的二进制解析 SHALL 在 PATH 扫描 `.exe` 失败后，解析 shim 定位 exe 真身并 spawn 真身（保持 stdout 管道与 kill 链路完好）。

#### Scenario: npm 全局安装后插件可用

- **WHEN** Windows 上 `npm install -g @xlight-oss/visionary-server` 完成（PATH 只有 shim），未配置 `binaryPath` / `DEEPSEEK_VISIONARY_BIN`，调用 `deepseek_vision`
- **THEN** 插件从 `visionary-server.cmd` / `.ps1` shim 解析出 `node_modules\.bin_real\visionary-server.exe` 真身并执行，返回分析结果

#### Scenario: shim 缺失或格式异常时回退友好错误

- **WHEN** 无法从 PATH 找到 `.exe` 或可解析的 shim
- **THEN** 工具返回平台化的安装指引错误（win32 给出 npm / binaryPath 指引），不崩溃

### Requirement: 二进制解析即时生效（懒解析）

插件 SHALL 在每次工具调用时重新解析二进制路径（而非 `apply()` 时缓存一次），使用户修改 PATH 或设置 `DEEPSEEK_VISIONARY_BIN` 后无需重启 DSH 即生效。

#### Scenario: 设置环境变量后立即生效

- **WHEN** 工具首次调用因找不到二进制而失败，随后用户设置 `DEEPSEEK_VISIONARY_BIN` 指向有效 exe，再次调用同一工具
- **THEN** 第二次调用成功，无需重启 DSH

### Requirement: 错误提示平台化

二进制缺失的提示信息 SHALL 按平台给出对应安装命令：win32 给 npm 全局安装 / binaryPath 指引；其他平台给 curl / brew / npm 命令。

#### Scenario: Windows 用户看到 Windows 安装指引

- **WHEN** win32 平台二进制缺失
- **THEN** 提示包含 `npm install -g @xlight-oss/visionary-server`、`Config.binaryPath`、`DEEPSEEK_VISIONARY_BIN` 指引，不含 Unix 专属命令

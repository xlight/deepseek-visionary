## ADDED Requirements

### Requirement: cargo-dist 多通道安装器
仓库 SHALL 接入 cargo-dist（`dist-workspace.toml` / `Cargo.toml [workspace.metadata.dist]`），在打 tag 发布时自动生成并发布安装器：shell 安装脚本（`curl -LsSf ... | sh`）、PowerShell 安装脚本、npm 包（postinstall 拉取二进制）、Homebrew tap formula。发布的 GitHub Releases 资产 SHALL 包含 cargo-dist 生成的 archive（tar.gz/zip）与安装器。发布 tag MUST 与 `Cargo.toml` 中的 workspace version 一致（cargo-dist 强制 `tag == v{version}`），发布前 MUST 先 bump 版本号。

#### Scenario: 打 tag 自动生成安装器
- **WHEN** 维护者将 workspace version bump 至与 tag 一致（如 0.2.0）后推送 `v0.2.0` tag
- **THEN** 发布流水线自动构建 5 平台二进制、打包 archive、生成 shell/powershell 安装器，并上传至 GitHub Releases

#### Scenario: tag 与版本不匹配
- **WHEN** 推送的 tag 与 `Cargo.toml` workspace version 不一致
- **THEN** cargo-dist 构建失败并明确报错提示 tag 与版本不匹配

#### Scenario: npm 安装
- **WHEN** 用户执行 `npm install -g <visionary-npm-package>` 或 `npx <visionary-npm-package>`
- **THEN** 安装对应平台的 `visionary-server` 二进制到 PATH，可执行 `visionary-server --version`

#### Scenario: Homebrew 安装
- **WHEN** 用户执行 `brew install <tap>/visionary-server`
- **THEN** 安装对应平台的 `visionary-server` 二进制到 PATH

### Requirement: MCPB 包保留
发布流水线 SHALL 继续生成并上传 MCPB 包（`visionary-server-<target-triple>.mcpb`，zip = manifest.json + 二进制），以保持 MCP Registry（`io.github.xlight/deepseek-visionary`）分发通道可用。cargo-dist 生成的流水线 MUST 追加 MCPB 构建步骤。

#### Scenario: Release 含 mcpb 资产
- **WHEN** 打 tag 发布完成
- **THEN** GitHub Releases 资产中包含每个目标平台的 `.mcpb` 文件，且 Registry 中的 server.json 下载地址仍然有效

### Requirement: Registry 元数据随版本更新
每次发布后仓库根目录的 `server.json` SHALL 被更新：`version` 字段与发布版本一致，5 个平台的 `fileSha256` 与本次发布的 `.mcpb` 实际哈希一致。发布流水线或发布清单 MUST 包含该更新步骤，防止 Registry 通道因哈希失配而静默失效。

#### Scenario: 发布后更新 Registry 元数据
- **WHEN** 新版本发布完成
- **THEN** `server.json` 的 `version` 与各平台 `fileSha256` 被更新为本次发布的实际值并提交，Registry 客户端可正常校验下载

### Requirement: 裸二进制资产保留（Zed 扩展兼容）
发布流水线 SHALL 继续上传各平台的裸二进制资产（命名 `visionary-server-<target-triple>`，Windows 带 `.exe`），以兼容 Zed 扩展壳 `visionary-zed-ext` 现有的"直接下载裸二进制"逻辑；Zed 扩展壳的下载/解压逻辑在本 change 中 MUST 保持不变。

#### Scenario: Release 含裸二进制资产
- **WHEN** 打 tag 发布完成
- **THEN** GitHub Releases 资产中包含每个目标平台的裸二进制文件，Zed 扩展壳无需改动即可下载使用

### Requirement: CI MCP smoke test
CI SHALL 在发布前对构建产物运行 MCP 协议 smoke test：基于 stdio 启动 `visionary-server`，完成 MCP initialize 握手，并断言工具列表包含 `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout` 四个工具。`scripts/mcp_probe.py` SHALL 提供 `smoke <binary-path>` 子命令并被接入为 CI 步骤；其原有 `analyze <image-path>` 测图模式 SHALL 保留。

#### Scenario: 发布前握手与工具列表校验
- **WHEN** 发布流水线构建完成且运行 `mcp_probe.py smoke <binary>`
- **THEN** 测试以 stdio 启动二进制、完成 initialize 握手、断言四个工具存在；任一失败则发布流程失败

#### Scenario: 本地手动探测
- **WHEN** 维护者执行 `python3 scripts/mcp_probe.py smoke <binary-path>`
- **THEN** 输出握手结果与工具列表，非零退出码表示协议或工具缺失问题

#### Scenario: 保留原有测图模式
- **WHEN** 维护者执行 `python3 scripts/mcp_probe.py analyze <image-path> [prompt]`
- **THEN** 保持现有行为：对指定图片调用 `deepseek_vision` 并输出结果与耗时

## Why

`visionary-server` 已是标准 MCP stdio server，但分发与接入只面向 Zed：README 只有 Zed 安装路径、无 `docs/` 目录、`main.rs` 纯 serve 无任何 CLI 子命令、安装渠道仅 GitHub Releases 手动下载。用户无法低成本地把这个 MCP 能力接入 opencode、Codex、Claude Code、Cursor 等主流 agent——每个 agent 的 MCP 配置形状不同（opencode 顶层 `mcp` 键 + `command` 数组、Codex `[mcp_servers]`、Claude/Cursor `mcpServers`），且缺少引导工具和一键安装脚本，通用化的最后一步（分发层 + 接入层）未完成。

## What Changes

- **新增 `visionary-server` CLI 子命令骨架**：无参数时保持现有 MCP stdio serve 行为（**BREAKING**：`main.rs` 引入参数解析，需保证 `command` 无参启动路径完全兼容所有现有客户端）；新增 `--version`、`doctor`、`init` 子命令
- **新增 `init` 引导子命令**（仿 docfork `dgrep` / mintlify `index` / vestige `init`）：检测已安装的 agent（opencode / codex / claude / cursor 等），按各 agent 配置形状写入 MCP 配置，写前备份原文件；支持 `--dry-run` 预览；opencode 配置显式写入 `timeout: 60000` 规避 npx 冷启动超时（默认 5000ms）
- **新增 `doctor` 诊断子命令**：输出 config 路径与权限、浏览器检测、token 状态（复用现有校验探针）、平台信息
- **新增 `docs/integrations/*.md` 文档矩阵**：opencode / Codex / Claude Code / Claude Desktop / Cursor / Zed 各一份接入文档，含 CLI 一键接入与手动配置两种路径
- **README 重构**：安装章节从"仅 Zed"改为"通用 MCP 接入"优先，Zed 扩展降为其中一个接入方式；新增每 agent 一键接入小节
- **引入 cargo-dist 分发**：替换手写 release.yml，自动生成 shell/powershell/npm/homebrew 安装器；保留 MCPB 包构建与上传（cargo-dist 不生成，需在生成的 workflow 追加）
- **CI 增加 MCP smoke test**：将现有 `scripts/mcp_probe.py` 接入 CI，发布前自动验证 MCP 协议握手与工具列表
- **Zed 扩展壳兼容性（待决策）**：cargo-dist 默认发布 tar.gz/zip archive，与 Zed 壳当前"下载裸二进制"逻辑冲突；方案 A 改壳下载 archive 解压，方案 B 保留双通道发布裸二进制 + archive

## Capabilities

### New Capabilities

- `server-cli`: `visionary-server` 的 CLI 入口行为——`--version` / `doctor` / `init` 子命令，无参数时进入 MCP stdio serve 模式
- `agent-onboarding`: `init` 引导工具（agent 检测、按形状写配置、备份、`--dry-run`）+ `docs/integrations/*.md` 接入文档矩阵
- `distribution`: cargo-dist 多通道安装器（shell/powershell/npm/homebrew）+ MCPB 包保留 + CI MCP smoke test

### Modified Capabilities

- `zed-extension`: 若采用方案 A（改壳下载 cargo-dist archive），`zed-extension` 的二进制获取逻辑从"下载裸二进制"变为"下载并解压 archive"；方案未定，待 design 决策后补 delta spec

## Impact

- **代码**：`crates/visionary-server/src/main.rs`（引入 clap 参数解析 + 子命令分发，无参路径保持 serve）；新增 `crates/visionary-server/src/cli/` 模块（init / doctor）
- **新依赖**：`clap`（CLI 解析）；`cargo-dist`（发布工具链，非运行时依赖）
- **工作流**：`.github/workflows/release.yml` 由 cargo-dist 生成（保留 mcpb 追加步骤 + 发布后更新 `server.json`）；`.github/workflows/ci.yml` 增加 MCP smoke test
- **版本管理**：cargo-dist 强制 `tag == v{workspace version}`，发布流程必须 bump `Cargo.toml` workspace version（当前 0.1.0 与 v0.1.6 tag 不匹配，属遗留问题）
- **脚本**：`scripts/mcp_probe.py` 子命令化（`smoke` / `analyze`）并接入 CI
- **文档**：`README.md` 重构；新增 `docs/integrations/*.md`
- **资产形态（待决策）**：GitHub Releases 资产从"仅裸二进制 + mcpb"变为"archive + mcpb（方案 A）"或"裸二进制 + archive + mcpb（方案 B）"
- **Zed 扩展壳**：`crates/visionary-zed-ext` 的下载/解压逻辑可能调整（取决于方案 A/B）

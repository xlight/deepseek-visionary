## Context

`visionary-server`（rmcp 2.2 标准 MCP stdio server）已具备通用 MCP 能力，但分发与接入只面向 Zed：README 仅 Zed 安装路径、无 `docs/` 目录、`main.rs` 纯 serve 无 CLI 子命令、安装渠道仅 GitHub Releases 手动下载 + MCP Registry mcpb。目标是把该能力低成本接入 opencode、Codex、Claude Code、Cursor 等主流 agent。

本 change 是纯增量：核心 vision 流水线（PoW → upload → fork → HIF → completion）与登录逻辑完全不动，只补"分发层 + 接入层"。

约束与现状（已核实）：
- 单二进制，PoW wasm 已 `include_bytes!` 内嵌，无运行时资产
- 发布流水线：手写 `.github/workflows/release.yml`（tag 触发，5 平台矩阵，产出裸二进制 + mcpb）
- MCP Registry 已注册 `io.github.xlight/deepseek-visionary`（依赖 mcpb 资产）
- 现有 specs：`auto-login` / `vision-analysis` / `zed-extension`
- 各 agent 配置形状差异：opencode 顶层 `mcp` 键 + `command` 数组；Codex `[mcp_servers]`（config.toml 或 `codex mcp add`）；Claude Code `claude mcp add` 或 `mcpServers`；Cursor / Claude Desktop `mcpServers`
- opencode `timeout` 默认 5000ms，npx 冷启动会超时

## Goals / Non-Goals

**Goals:**
- `visionary-server` 支持 CLI 子命令（`--version` / `init` / `doctor`），无参数路径与现状完全兼容
- `init` 引导工具：检测 agent → 按形状写配置 → 备份 → `--dry-run` 预览
- `docs/integrations/*.md` 文档矩阵 + README 通用化
- cargo-dist 多通道安装器（shell / powershell / npm / homebrew），保留 mcpb 与裸二进制资产
- CI 接入 MCP smoke test（复用 `scripts/mcp_probe.py`）

**Non-Goals:**
- 不改核心 vision 流水线与登录逻辑
- 不做远程/HTTP MCP server（保持 stdio-only）
- 不引入 OAuth 或其他鉴权
- 不改变 Zed 扩展壳的下载/解压逻辑（见决策 4，方案 B）
- `init` 仅覆盖 6 个 agent（opencode / codex / claude / claude-desktop / cursor / zed）；vscode、windsurf 等仅入文档矩阵，不写引导

## Decisions

### 决策 1：CLI 框架用 clap（derive），无参数路径保持 serve

`visionary-server` 引入 `clap`（derive 风格）作为 CLI 解析，根命令无子命令参数时直接进入现有 MCP serve 流程。

- **为什么 clap**：社区 MCP server（hmem、docfork 等）的标准做法；derive 风格声明式、错误提示友好（未知子命令自动输出 usage 与退出码）
- **为什么无参必须 serve**：所有 agent 的配置都是 `command: ["visionary-server"]` 无参启动，任何参数解析回归都会破坏全部现有接入
- **备选**：手写 `std::env::args` 解析——省一个依赖但错误处理、`-V` 等细节都要自己写，收益低
- **防护**：`main.rs` 中 `let cli = Cli::parse(); if cli.command.is_none() { serve().await } else { match ... }`；用 `mcp_probe.py` 对无参路径做回归断言

### 决策 2：init 采用"agent 自有 CLI 优先，直接改配置兜底"混合策略

| Agent | 首选方式 | 兜底方式 |
|-------|---------|---------|
| Claude Code | `claude mcp add --transport stdio deepseek-visionary -- visionary-server`（`--scope user`，非交互） | 写用户级配置 `mcpServers` |
| Codex | `codex mcp add deepseek-visionary -- visionary-server`（`--` 分隔符形式，vestige / sub-agents / better-context 多来源验证） | 写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]` |
| opencode | 直接写配置文件（无 add CLI） | — |
| Cursor | 直接写 `.cursor/mcp.json` / 用户级配置 | — |
| Claude Desktop | 直接写 `claude_desktop_config.json` | — |

- **为什么混合**：agent 自带 CLI 是最权威、最不易错的写入通道（自动处理配置文件位置与格式）；但 opencode / Cursor 无 add 命令，只能直接写文件
- **opencode 形状**：顶层 `mcp` 键，`type: "local"`，`command: ["visionary-server"]`，**显式 `timeout: 60000`**（官方默认 5000ms，冷启动超时是真实坑；vestige 官方接入文档同款值）
- **Codex 关键点**：配置键必须是 `mcp_servers`（`mcp.servers` 会静默失效，GitHub issue #3441 已验证）；CLI 写法为 `codex mcp add <name> -- <command>`（`--` 分隔，非 `--command` flag）
- **多 agent 批量（社区范式）**：mintlify（`npx mint index --claude --cursor --yes --project`）与 dgrep（`dgrep setup --claude --opencode`）均采用 flags 多选 + `--yes` 免交互。`init` 除位置参数 `init <agent>` 外，SHALL 支持 `--opencode/--codex/--claude/--cursor/--claude-desktop` 多选 flags 与 `--yes`（不提示直接写），对齐社区主流
- **写前备份**：所有直接改文件的路径，先复制 `*.bak.<UTC时间戳>`，备份失败即中止
- **JSON 合并策略**：读入 → 解析（失败即中止并报错，绝不覆盖）→ 只增改 `deepseek-visionary` 键 → 保留其余全部内容 → 写回
- **`--dry-run`**：只打印将写入的 JSON/TOML 片段与目标路径，不落盘

### 决策 3：doctor 复用现有校验探针

`doctor` 输出：config 路径 + 权限、浏览器检测（复用 `browser.rs` 的浏览器发现逻辑）、凭据状态（复用 `AuthManager::validate()` 的本地检查 + 真实 token 校验探针）、平台/架构、`--version`。严重失败（无浏览器且无凭据）退出非零。

- **为什么复用探针**：status 工具已有真实校验能力（`create_pow_challenge`），doctor 直接调用同一函数，保证"工具说有效"与"doctor 说有效"一致
- **实现前置**：真实探针（`upload::create_pow_challenge`）当前内联在 `server.rs` 的 `deepseek_vision_status` 工具处理器中，需先提取为共享函数（如 `auth::probe_token()`），status 工具与 doctor 共用；doctor 探针带超时避免网络卡死
- **输出格式（社区范式）**：ironclaw（`doctor` 展开为 DB/LLM/密钥等逐项 ✓/✗/⚠ + 可执行建议）与 mcpproxy-go（`doctor` 命名遵循 Homebrew 惯例，逐项健康检查 + 退出码）为成熟范本。doctor 输出对齐该模式：逐项 `✓/✗/⚠` + 失败项附修复建议，退出码非零表示存在严重失败；未来可扩展 `doctor --fix`（ironclaw 已实现）

### 决策 4：cargo-dist 接入采用**方案 B**——保留裸二进制与 mcpb 双通道，Zed 壳不动

cargo-dist（0.32.x）生成 `release.yml` 替代手写流水线，自动产出 shell/powershell/npm/homebrew 安装器与 archive；**追加自定义步骤**继续上传各平台裸二进制（`visionary-server-<target-triple>`）与 mcpb。

- **为什么方案 B 而非方案 A（改 Zed 壳下载 archive）**：Zed 壳是 wasm32-wasip2 薄壳，在 wasm 里解压 tar.gz 需要引入 tar/flate 依赖并在 wasp2 目标上验证，复杂度与回归风险高、收益低；保留裸二进制通道对 Zed 壳零改动、对 Registry mcpb 也零改动
- **实现要点**：cargo-dist 0.32 原生支持 `[[dist.extra-artifacts]]`（`artifacts` + `build` 键，官方配置文档已核实）——优先用配置声明裸二进制与 mcpb 的构建/上传，避免手改 workflow；仅当配置无法覆盖时用 workflow 追加步骤（spike 4.1 实测确认）
- **备选方案（已评估）**：`taiki-e/upload-rust-binary-action`（tokio 生态发布工具作者）可替代 cargo-dist——跨编译 + 上传裸二进制更简单，但**不产出 shell/npm/homebrew 安装器**，无法覆盖多通道目标；`cargo-binstall` 对 cargo-dist 的 GitHub Release URL schema 自动识别，接入 cargo-dist 后 `cargo binstall visionary-server` 免费获得
- **新兴渠道（顺势可用）**：Microsoft APM（`apm install --mcp <id>`）识别与 MCP Registry 相同的 `io.github.*` 标识，`io.github.xlight/deepseek-visionary` 注册后 `apm install --mcp io.github.xlight/deepseek-visionary` 可直接安装——无需额外建设，作为文档提及即可
- **版本管理（关键前置）**：cargo-dist 强制 `tag == v{Cargo.toml workspace version}`。当前 workspace version 为 `0.1.0` 而 release tag 为 `v0.1.6`，迁移后 MUST 先 bump version 至与 tag 一致（如 v0.2.0 对应 version 0.2.0），否则 cargo-dist 直接报错
- **发布流程协调**：以 cargo-dist 生成的 release 创建机制为准（避免与现有 softprops/action-gh-release 重复创建），裸二进制 + mcpb 追加到同一 release；`server.json`（MCP Registry 元数据，含 5 平台 mcpb 的 `fileSha256`）在每次发布后同步更新并提交
- **风险对冲**：若 cargo-dist 生成物与现有流程冲突，保留旧 `release.yml` 于 git 历史，可一键回滚

### 决策 5：CI smoke test 复用 mcp_probe.py（子命令化）

`scripts/mcp_probe.py` 升级为双子命令 CLI：`smoke <binary>`（stdio initialize 握手 + 断言 4 工具）与 `analyze <image> [prompt]`（保留现有测图逻辑），接入 release workflow 的 per-target 构建后与 PR 级 `ci.yml`。

- **为什么子命令化而非改 argv[1]**：现有脚本的 `argv[1]` 是图片路径（调用 `deepseek_vision` 做真实识图），若直接改成二进制路径会破坏现有测图工作流；`smoke` 与 `analyze` 两个子命令各司其职
- **为什么在 release workflow 内做**：裸二进制在 release 时才构建；PR 级 CI 用 host 平台二进制做快速回归即可

## Risks / Trade-offs

- **[clap 引入破坏无参 serve]** → 结构上保证无参走 serve 分支；`mcp_probe.py` 在 CI 对无参路径做握手断言；本 change 的验收标准包含"现有 Zed 扩展 + 手动配置全部照常工作"
- **[直接写配置文件破坏用户现有配置]** → 备份先行 + 严格 JSON 解析失败即中止 + 只增改单一键；文档明确"先备份，`--dry-run` 预览"
- **[cargo-dist 生成 workflow 与 mcpb/裸二进制追加步骤冲突]** → 先 `cargo dist plan` spike 验证；旧 release.yml 保留于 git 历史可回滚；发布用新 tag（v0.2.0）验证
- **[opencode 配置实际是 JSONC 而非严格 JSON]** → 解析失败时中止并提示手动编辑，不猜测
- **[npm 包名 / homebrew tap 名冲突]** → 用 scoped 包名 `@xlight-oss/visionary-server`（npm 组织 xlight-oss，配置键为 `[dist] npm-scope`）与独立 tap 仓库 `xlight/homebrew-tap`
- **[claude mcp add 交互提示挂起]** → 使用 `--scope user` 与非交互参数；超时降级为直接写配置
- **[version bump 遗漏导致 cargo-dist 发布失败]** → 把"bump workspace version 并同步 server.json"纳入发布检查清单；发布流程首步即校验 `tag == v{version}`

## Migration Plan

1. **阶段 1（CLI）**：合入 clap + `--version` / `doctor` / `init`（无参路径不变）→ 跑 `cargo test -p visionary-server` 与 `mcp_probe.py` 回归
2. **阶段 2（文档）**：`docs/integrations/*.md` + README 通用化
3. **阶段 3（分发）**：`cargo dist init` spike → 生成 workflow + 追加裸二进制/mcpb 步骤 → 打 v0.2.0 验证 5 平台 + npm/homebrew 发布
4. **回滚**：CLI 阶段为纯增量，可 revert；分发阶段 revert workflow 文件即可，旧流程在 git 历史中完整保留

## Open Questions

- npm 包名与 scope（实施时查 npm registry 可用性）
- Homebrew tap 仓库名与归属（新建独立 repo 或并入现有组织）
- ~~cargo-dist 0.32 对 extra-artifacts 的自带支持~~（已确认：`[[dist.extra-artifacts]]` 原生支持，见决策 4）
- ~~`init` 是否需要非交互 `--yes` 模式~~（已定：社区 mintlify/dgrep 均为 flags 多选 + `--yes` 范式，纳入决策 2）
- mcpb manifest 防漂移：n8n-mcp 用脚本从实时工具注册表生成 manifest 防版本/工具漂移；本项目 `build_mcpb.py` 从 `GITHUB_REF_NAME` 取版本、工具列表静态，漂移风险低——是否值得对齐 n8n 的实时生成（倾向：暂不，观察工具列表变化频率）

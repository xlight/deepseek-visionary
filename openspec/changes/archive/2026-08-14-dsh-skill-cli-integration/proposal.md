# dsh-skill-cli-integration

## Why

DeepSeek Harness（DSH，`dsh`）是 DeepSeek 官方的 agent harness，其模型可执行 shell 命令，并默认扫描 `~/.agents/skills` 与 `~/.dsh/skills` 作为技能根。项目现有能力（CLI `vision` 子命令 + 内嵌 `skill install`）天然契合 DSH：装上二进制 + 装好 skill 即可让 DSH 中的 agent 识图。同时 DSH 的插件体系（`dsh plugin` + bundle 自注册）是 DSH 生态视觉能力的原生主流形态（社区已有 dsh-vision-toolkit / dsh-plugin-deepeye），发布原生插件包可让 DeepSeek Visionary 以原生工具（结构化 schema、宿主级权限）进入 DSH，并规避 bash 沙箱对会话持久化的限制。目前 DSH 未列入受支持 agent，这两条路径用户都无从发现。

## What Changes

- `init` 子命令新增 `dsh` agent（`--dsh` 批量 flag）：
  - 检测 DSH（`dsh` 在 PATH / `$DSH_HOME` 环境变量 / `~/.dsh/profiles` 目录存在）
  - 复用 `skill install` 的写入逻辑，将内嵌 SKILL.md 安装到 DSH 技能发现根：`$DSH_HOME/skills/visionary-cli/SKILL.md`（user-dsh 根）与 `~/.agents/skills/visionary-cli/SKILL.md`（user-agents 根，DSH 默认扫描）
  - 不写任何 MCP 配置（skill + CLI 轻量路线）；`--dry-run` 预览不落盘
- **新增 DSH 原生插件包** `packages/dsh-plugin/`（npm 分发，纯 ESM 无构建）：
  - 声明 `dsh.bundle.patch` + 自带 `cordis.patch.yml` 自注册 → 用户 `dsh plugin --profile <name> add @xlight-oss/visionary-dsh` 即可安装，零手写配置
  - 宿主侧 Cordis 插件，经 `ctx.tools.register` 注册 4 个原生工具（`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`，命名与 MCP 对齐），工具内部 spawn `visionary-server <cmd> --json` 复用 Rust 管道（PoW → 上传 → fork → HIF → SSE），不重复实现视觉流水线
  - 原生工具在 DSH 宿主进程执行、不经 bash 沙箱，`--continue` 续聊（写 `~/.deepseek-visionary/session.json`）与 `login`（起浏览器）不受工作区写限制
- 新增接入文档 `docs/integrations/deepseek-harness.md`：插件安装（推荐）、skill + CLI 手动路径、DSH 中实际调用方式（模型经 bash 调 `visionary-server vision <image> --json`）、技能发现根说明、验证与常见问题；说明 DSH 沙箱下 `--continue` 续聊与会话持久化、`login` 的约束；附"进阶：MCP 模式"简短说明（DSH 通过 `dsh-mcp-client` + `cordis.patch.yml` 支持 MCP，作为可选通道）
- README 更新：支持的 agent 列表、接入表格、CLI + skill 章节补充 DSH、插件安装入口
- `docs/cli.md` 的 `init` 行与 agent 列表同步更新
- 测试：`init dsh` 解析、DSH 检测、skill 写入两技能根、`--dry-run` 不落盘

## Capabilities

### New Capabilities

- `dsh-plugin`: DeepSeek Visionary 的 DSH 原生插件包 —— 经 `dsh.bundle.patch` 自注册的 Cordis bundle，注册 `deepseek_vision` 等原生工具（spawn `visionary-server --json` 复用 Rust 管道），二进制解析（PATH / config / env）与错误处理

### Modified Capabilities

- `agent-onboarding`: `init` 子命令新增 `dsh` agent —— 检测 DeepSeek Harness、经 skill 安装完成接入（不写 MCP 配置）；接入文档矩阵新增 deepseek-harness.md
- `cli-commands`: `skill` 子命令的写入逻辑抽为可复用安装函数，供 `init dsh` 向 DSH 技能根（`$DSH_HOME/skills`）安装同一内嵌 SKILL.md（`skill install` 本身行为不变，仍写 `~/.agents/skills/visionary-cli/SKILL.md`）

## Impact

- `crates/visionary-server/src/cli.rs`：`InitArgs` 新增 `--dsh` flag；help 文案与错误提示的 agent 列表加 `dsh`
- `crates/visionary-server/src/onboarding.rs`：`Agent` 枚举加 `DeepseekHarness`；DSH 检测（PATH / `$DSH_HOME` / `~/.dsh`）；`init dsh` 写入逻辑（复用 skill 安装）；`$DSH_HOME` 解析辅助函数
- `crates/visionary-server/src/cli.rs`：`cmd_skill` 抽出 `install_skill` 帮助函数供 onboarding 复用
- **新增** `packages/dsh-plugin/`（纯 ESM，无构建）：`package.json`（`dsh.bundle.patch` 声明）、`cordis.patch.yml`、`lib/index.mjs`（4 个原生工具）、README；发布走 npm publish（独立于 cargo-dist，`@xlight-oss/visionary-dsh`）
- 文档：`README.md`、`docs/cli.md`、新增 `docs/integrations/deepseek-harness.md`
- Spec：`openspec/specs/agent-onboarding/spec.md`、`openspec/specs/cli-commands/spec.md`、新增 `openspec/specs/dsh-plugin/spec.md`
- 依赖：Rust 侧无新增（skill + CLI 路线不需要 YAML/JSON 配置写入）；插件包 peerDependencies `@deepseek-ai/cordis` / `@deepseek-ai/dsh-tools`（由 DSH profile 提供）

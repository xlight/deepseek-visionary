# dsh-skill-cli-integration Design

## Context

项目定位为"CLI + skill 优先"（见已归档 mcp-stdio-subcommand 变更）：`visionary-server` 二进制内嵌 agent 调用契约 SKILL.md，`skill install` 将其写入 `~/.agents/skills/visionary-cli/SKILL.md`。DeepSeek Harness（DSH）的技能文件系统 provider（`dsh-skill-filesystem`）默认扫描多个技能根，其中 user 级根为 `$DSH_HOME/skills`（rank 400）与 `$DSH_AGENTS_HOME`/`~/.agents/skills`（rank 500，`$DSH_AGENTS_HOME` 未设置时默认 `~/.agents`），且 DSH 模型可执行 shell 命令（bash tool）。因此现有 `skill install` 产物已天然被 DSH 发现，接入 DSH 无需 MCP 配置、无需新增依赖。

同时，DSH 的插件体系（`dsh plugin --profile <name> add <pkg>` + `dsh.bundle.patch` 自注册）是 DSH 生态原生能力的标准形态：社区视觉插件（dsh-plugin-deepeye、dsh-vision-toolkit）均注册 `ctx.tools` 原生工具。原生工具在宿主进程执行（不经 bash 沙箱），可写 `~/.deepseek-visionary/` 并启动浏览器，从而规避 skill + CLI 路径下 bash 沙箱对会话持久化的限制（见 Round 1 缺陷 1）。本设计在 skill + CLI 基础上增加原生插件包作为 DSH 内的推荐路径。

## Goals / Non-Goals

**Goals:**
- `init dsh` 一键接入：检测 DSH、将内嵌 SKILL.md 安装到两个 DSH 技能根、汇总输出
- 发布 DSH 原生插件包 `@xlight-oss/visionary-dsh`：`dsh plugin add` 一键安装、4 个原生工具、结构化 schema、宿主级权限（续聊/登录不受沙箱限制）
- 接入文档与 README 覆盖 DSH（插件路径 + skill + CLI 路径），`skill install` 行为保持向后兼容（仍写 `~/.agents/skills`）

**Non-Goals:**
- 不实现 DSH 的 MCP 通道（DSH 支持 MCP 需在 `cordis.patch.yml` 加 `dsh-mcp-client` 插件行；本文档仅在 deepseek-harness.md 中作为"进阶"附简短手动说明，不写代码）
- 插件不重复实现视觉流水线（PoW / 上传 / fork / HIF / SSE）——spawn `visionary-server` 复用 Rust 实现，JS 侧仅做参数映射与 JSON 解析
- 插件不写 Client 半（无 Web UI 卡片/Slot）；纯工具注册，留待后续版本
- 不改动 SKILL.md 内容（现有 frontmatter `name: visionary-cli` / `description` 已满足 DSH 技能发现契约：kebab-case 名称 + 描述，一级目录 `<root>/<name>/SKILL.md`）
- 不向项目级技能根（`<projectRoot>/.dsh/skills`）安装——DSH 接入是用户级能力，与项目无关

## Decisions

### 决策 1：agent 名用 `dsh`，接受别名 `deepseek-harness`

- **选择**：`Agent::DeepseekHarness`，`name()` 返回 `"dsh"`；`from_name` 接受 `"dsh"` / `"deepseek-harness"` / `"deepseek_harness"` / `"harness"`。批量 flag 为 `--dsh`。
- **备选**：`deepseek-harness` 作为唯一名称——与官方 CLI 命令 `dsh` 不一致，且位置参数输入过长。
- **理由**：与 DSH 官方 CLI 命名一致（其余 agent 均用 CLI 短名：opencode / codex / claude / cursor）；别名保证文档/用户直觉写法都能命中。

### 决策 2：DSH 根目录解析 `$DSH_HOME` 优先，回退 `~/.dsh`

- **选择**：新增纯函数 `dsh_home_dir(home: &Path) -> PathBuf`：读 `DSH_HOME` 环境变量（非空时用之），否则 `home.join(".dsh")`。与 DSH 官方 `dsh-home-paths` 解析规则一致。
- **备选**：仅用 `~/.dsh`——忽略 `$DSH_HOME` 自定义安装，检测与写入会指向错误位置。
- **理由**：DSH 官方约定 `$DSH_HOME` 可覆盖默认根；检测与安装必须用同一解析结果，避免"检测到 A 路径、写入 B 路径"。

### 决策 3：检测条件为 `dsh` 在 PATH 或 `$DSH_HOME`/`~/.dsh` 存在

- **选择**：`dsh` 可执行文件在 PATH（复用现有 `find_in_path`）**或** `dsh_home_dir(home)` 下存在 `profiles` 目录（任一 profile 即视为已安装）。`config_path` 报告为 DSH 根目录本身。
- **备选**：仅检测 PATH 中的 `dsh` 二进制——通过 npm/npx 或便携方式运行的 DSH（如本机 `~/.npm/_npx/.../dsh` 不在 PATH）会被漏检。
- **理由**：`~/.dsh/profiles/*` 存在即证明 DSH 初始化过（`dsh web` / `dsh --profile headless` 首次运行自动创建），覆盖面最大且无副作用。

### 决策 4：`init dsh` 写两个技能根，`skill install` 行为不变

- **选择**：把 `cmd_skill` 中"创建目录 + 写内嵌 SKILL.md + 返回路径"抽为 `install_skill(dir: &Path) -> Result<PathBuf>`（`cli.rs` 或 onboarding 内共享）。`cmd_skill` 保持只写 `~/.agents/skills/visionary-cli/`；`init dsh` 调用两次：`$DSH_HOME/skills/visionary-cli/` 与 `~/.agents/skills/visionary-cli/`。`init dsh --dry-run` 只打印两个目标路径，不落盘。
- **备选 A**：`skill install` 检测到 DSH 就多写一份——给非 DSH 用户引入隐式行为，`skill install` 输出与既有 spec 契约偏离，否决。
- **备选 B**：`init dsh` 依赖用户先跑 `skill install`——多一步，且只覆盖 `~/.agents/skills` 一个根，否决。
- **理由**：两根写入保证无论 DSH 的 `agentsHome` 是否被改过，`$DSH_HOME/skills`（始终扫描的 user-dsh 根）都能发现 skill；`init dsh` 一次性完成。

### 决策 5：`init dsh` 不要求 DSH 已检测到才能写

- **选择**：与现有 agent 写入路径一致——显式 `init dsh` 直接写两个技能根并汇总；DSH 是否"已安装"只影响交互式检测列表的标记。`--dry-run`/`--yes` 语义与其余 agent 完全一致。
- **理由**：skill 安装本身无害；用户可能在另一台机器/容器里跑 DSH，本机只准备 skill。

### 决策 6：写入不解析/改写现有 YAML，纯追加式文件写入

- **选择**：`init dsh` 只创建/写入 `SKILL.md` 文件（`std::fs` 创建目录 + 写文件，覆盖已存在），不触碰 DSH 的任何配置（`cordis.patch.yml` / `settings.yaml`）。
- **备选**：写入 cordis.patch.yml 的 `dsh-mcp-client` 插件行——需要 YAML 解析/改写（现有依赖无 YAML crate），且用户已明确偏好 skill + CLI 轻量路线，否决。
- **理由**：零依赖、零配置破坏风险；DSH 技能发现完全由 DSH 侧完成。

### 决策 7：发布 DSH 原生插件包，工具经 spawn CLI 复用 Rust 管道（Round 2）

- **选择**：新增 `packages/dsh-plugin/`，npm 包名 `@xlight-oss/visionary-dsh`（与既有 `@xlight-oss/visionary-server` 同 scope）。包为纯 ESM 普通 JavaScript（无 TS/构建步骤），声明 `dsh.bundle.patch` 指向自带 `cordis.patch.yml`（`- insert:` 自注册），`lib/index.mjs` 导出 Cordis 插件（`name` / `inject: ["tools"]` / `Config` / `apply`），经 `ctx.tools.register(defineTool(...))` 注册 `deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`。每个工具 `execute` 用 `child_process.spawn` 调 `visionary-server <cmd> --json` 并解析原子 JSON。
- **备选 A**：JS 直调 DeepSeek 网页 API——需要复刻 PoW/上传/fork/HIF 签名/SSE 流水线（~1500 行 Rust 的验证成果），重复造轮子，否决。
- **备选 B**：TypeScript + tsdown 构建（deepeye 模式）——repo 无 Node 工具链，插件仅 ~300 行薄包装，纯 ESM 更轻，否决。
- **备选 C**：插件内嵌 `dsh-mcp-client` 行复用 MCP——与文档已给出的手动 MCP 进阶路径重复，且无原生 schema 收益，否决。
- **理由**：薄包装复用已验证的 Rust 管道（独特约束：零 API key、免费 DeepSeek 网页视觉模型）；原生工具获得结构化 schema + 宿主级权限（不经 bash 沙箱 → 续聊/登录不受工作区写限制，消解 Round 1 缺陷 1）；纯 ESM 零构建贴合轻量原则。

### 决策 8：原生工具命名与 MCP 对齐，二进制解析 PATH → config → env

- **选择**：工具名沿用 MCP 工具名（`deepseek_vision` 等），跨 agent 命名一致。二进制解析：`Config.binaryPath` → `DEEPSEEK_VISIONARY_BIN` → PATH 查找；缺失时工具返回含安装指引的错误（不崩溃、不阻断 DSH）。
- **备选**：插件内联二进制下载——引入网络与平台矩阵复杂度，与 cargo-dist/brew/npm 安装通道重复，否决。
- **理由**：命名一致性降低模型迁移成本；解析顺序可配置可兜底，错误信息指向既有安装通道。

### 决策 9：插件不写 Client 半，系统提示仅精简引导

- **选择**：本轮插件仅宿主侧（工具注册），不实现 Client/Slot/Web 卡片；`ctx.systemPrompt.section` 注入精简引导（工具存在 + 使用场景），不重复 schema。
- **备选**：实现 Web 工具卡片（deepeye 有 `presentCall`/`presentResult` 卡片）——增强展示但增加维护面，留待后续版本。
- **理由**：工具无需 Client 即可被模型调用；先保证能力闭环，展示层增量后置。

### 决策 10：工具声明 timeoutMs 并转发 exec.signal；结果以 stdout JSON 为准（Round 3）

- **选择**：每个工具在 `ToolDefinition` 声明 `timeoutMs`（login = `loginTimeoutSeconds × 1000`，`loginTimeoutSeconds` 默认读取 `DEEPSEEK_LOGIN_TIMEOUT` env、未设置时 600，与 CLI 自身登录超时一致；vision 默认 300000ms；status/logout 默认 60000ms），`execute` 将 `exec.signal` 转发至 spawn 的子进程（abort → kill）。工具解析以 CLI stdout 原子 JSON 为准而非退出码：`status --json` 未登录时输出完整 JSON 但退出码非零，仍解析展示；`vision --json` 失败以退出码 + `{"error"}` 判定。
- **依据**（DSH 源码核实）：dsh-tool-call-timeout-policy 只对声明 `timeoutMs` 的工具施加 deadline，且声明即承诺协作转发 `exec.signal`（未转发则超时不生效）；`status --json` 非零退出但 stdout 完整是 CLI 既有约定（cli.rs `cmd_status`）。
- **理由**：login 阻塞（最长 600s）必须有界且可取消；status 按 JSON 解析避免未登录被误报为工具失败；与 DSH 官方 web 工具（web_fetch/web_search）的 timeoutMs + signal 转发模式一致。
- **并存引导**：`ctx.systemPrompt.section` 同时说明若装有 visionary-cli skill 应优先原生工具（宿主级执行，续聊/登录不受 bash 沙箱限制）。

### 决策 11：输入边界——按子命令参数面调用、等号传参、base64 走临时文件（Round 4）

- **选择**：
  - `login` / `logout` 子命令**无 `--json` 参数**（cli.rs `Command::Login`/`Logout` 无 args；实测 `login --json` → clap 报错），插件按子命令真实参数面调用：`vision` / `status` 用 `--json` 原子解析，`login` / `logout` 直接透出文本输出
  - 选项传参一律等号形式 `--prompt=<value>` / `--session-id=<value>`（实测空格形式 `--prompt --help` 被 clap 拒绝、等号形式正常）
  - `deepseek_vision` 的 base64 / data URI 入参先解码写临时文件（`os.tmpdir()`）再传路径，调用后清理——Linux 单参数上限 131072 字节，真实截图 base64 会 E2BIG；本地路径直传
- **依据**（真实二进制验证）：`login --json` / `logout --json` → `unexpected argument`；`--prompt --help`（空格）→ `a value is required`；`--prompt=--help`（等号）→ 正常解析。
- **理由**：按子命令实际参数面调用是正确性底线；等号传参消除 clap hyphen 值歧义；临时文件方案兼容任意大小图片且不污染调用方文件系统。

### 决策 12：版本探测警告与上传安全提示（Round 5）

- **选择**：插件在 apply 时探测 `visionary-server --version` 并记录；与插件声明兼容版本不匹配时工具结果附版本警告（不阻断）。`deepseek_vision` 的 `image` 提示为会读取并上传的文件（文档 FAQ 落实）。
- **依据**：插件与二进制经 `--json` 输出形状耦合（design Risk），版本漂移难排查；image 路径在宿主级权限下会被读取上传，属安全提示义务（security > correctness）。
- **理由**：探测成本极低（一次 `--version`），提前暴露版本不匹配；上传提示让用户在明示下选择路径，缓解提示注入外传风险。

## Risks / Trade-offs

- [用户 DSH 改过 `$DSH_AGENTS_HOME`] → `~/.agents/skills` 根可能不被扫描；`$DSH_HOME/skills`（始终扫描）兜底，文档注明两个根。`write_dsh` 不感知 `$DSH_AGENTS_HOME`（固定写 `~/.agents`）为有意取舍：与 `skill install` 契约一致，且兜底根已保证发现
- [DSH 沙箱下会话持久化被拒] → 仅影响 skill + CLI 路径：每次成功 `vision` 都会写 `~/.deepseek-visionary/session.json`（`pipeline.rs` → `session.rs`，写失败被静默吞掉）；DSH bash 沙箱 `workspace-write` 只允许写 workspace + /tmp，该写会被拒。单次识图不受影响（读图/网络不受限）；`--continue` 续聊不跨调用持久。文档 FAQ 说明约束（续聊需 `danger-full-access` 或接受单次；`login` 建议在终端执行或注入 `DEEPSEEK_USER_TOKEN`）。**插件路径（决策 7）在宿主进程执行，不受此限制**
- [插件与二进制版本耦合] → 插件依赖 `visionary-server` 的 `--json` 输出形状；二进制在 PATH 之外的版本差异由 `binaryPath` 配置兜底；文档注明两者同源发布
- [DSH 技能缓存/运行时未刷新] → 文档注明"重启 DSH 或等待技能目录热加载后生效"；DSH 对技能根有 Chokidar 观察，新增文件会被发现
- [`visionary-server` 不在 DSH 进程 PATH] → 沿用 `init` 既有 PATH 预检；插件侧 `binaryPath` / `DEEPSEEK_VISIONARY_BIN` / PATH 三级解析；文档常见问题给出绝对路径方案
- [用户已有 `$DSH_HOME/skills` 下同名目录] → 覆盖写入并提示"已更新"，与 `skill install` 语义一致

## Migration Plan

无存量迁移：新增 agent、新增文档与新增插件包，不改变既有命令行为。`skill install` 输出不变。发布后：`visionary-server init dsh`（skill + CLI 路径）或 `dsh plugin --profile web add @xlight-oss/visionary-dsh`（原生插件路径）均可接入。

## Open Questions

- 插件包发布通道：已定——`npm publish` 独立于 cargo-dist 流水线，并纳入 GitHub Actions release：新增 `.github/workflows/dsh-plugin-release.yml`，push `v*` tag 时与 cargo-dist / Zed 扩展同步触发，先校验版本一致性（tag == package.json == workspace Cargo.toml）再 `pnpm publish`（需仓库配置 `NPM_TOKEN` secret）。
- `deepseek_vision_login` 阻塞语义：已由决策 10（timeoutMs 声明 + signal 转发）约束为有界、可取消；非阻塞变体（后台 spawn + `deepseek_vision_status` 轮询）仍为可选增强，实现轮验证后再定，不影响 spec。

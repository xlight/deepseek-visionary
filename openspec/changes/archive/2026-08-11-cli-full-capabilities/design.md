## Context

`visionary-server` 目前能力分两层：MCP 工具层（`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`，经 `rmcp` 暴露）与 CLI 层（`--version` / `doctor` / `init`，clap 解析）。核心实现（vision 流水线、CDP 登录、token 探针）已经是与 MCP 无关的普通函数（`pipeline.rs` / `login.rs` / `auth.rs`），但有两处 MCP 类型泄漏阻碍 CLI 复用：

- `login.rs::run_login` / `run_logout` 返回 `rmcp::model::CallToolResult`
- `deepseek_vision` handler 内联了会话续聊解析与图片读取逻辑，未抽离

约束：无参数启动必须保持 MCP stdio serve 行为（design.md 决策 1，`generalize-mcp-distribution` change），stdout 是 MCP 协议通道不可污染，日志必须走 stderr。

## Goals / Non-Goals

**Goals:**
- CLI 覆盖与 MCP 工具等价的 4 个能力：`vision` / `status` / `login` / `logout`
- `vision` 支持流式输出（终端手感）与 `--json` 结构化输出（脚本/agent 消费）
- MCP 工具行为完全不变，核心逻辑单一实现（无两套漂移代码）
- 失败退出非零，便于脚本判断

**Non-Goals:**
- `session` / `config` 管理子命令（用户明确排除）
- 合并 `status` 与 `doctor`（两者并存：`status` 轻量鉴权、`doctor` 完整环境诊断）
- 多图同时分析（单图 + 会话续聊已覆盖对比需求）
- 非 TTY 时自动切换输出格式（保持显式 `--json`，不做隐式行为）

## Decisions

### 决策 1：核心层返回值去 MCP 化

`login.rs::run_login` / `run_logout` 改为返回 `Result<String>`（人类可读文本），`server.rs` 的 MCP handler 负责包装成 `CallToolResult`。

- **备选**：返回结构化 `Credentials` / `LoginOutcome` 再由各层格式化。否决——CLI 与 MCP 输出文案本就一致（中文提示），文本返回最简，格式化逻辑留在 `login.rs` 一处。
- **收益**：CLI 直接 `println!("{}", run_login(&config).await?)`，零适配。

### 决策 2：流式回调——单函数 + 可选回调

`completion::vision_completion` 增加可选参数 `on_token: Option<&mut dyn FnMut(&str)>`，`parse_sse` 解析到 content delta 时先回调再收集；`pipeline::run_vision_pipeline` 透传。

**接线点（代码核对后简化）**：`parse_sse`（completion.rs）本来就是逐行收集——解析到 `v` 字符串（`text_parts.push(v)`）或 `type=text`（`text_parts.push(joined)`）时同步调用 `on_token` 即可。**无需双分支**：`--json` 模式传 `None` 只用返回值，流式模式传 `Some(print)`；同一个函数、可选回调，收集逻辑天然复用。

- **备选**：`tokio::sync::mpsc` channel 流式返回。否决——流水线目前是同步返回 `(String, Option<String>)` 的单函数，引入 channel 需改造成 stream 接口，波及 MCP 路径且无收益；回调是侵入最小的透传。
- **风险**：回调在异步上下文中是同步闭包，打印到 stdout 会阻塞事件循环。`vision` 子命令是独立进程、单请求，阻塞可接受；若未来并发请求再考虑 channel。

### 决策 3：输出模式——TTY 感知默认 + 显式开关，`--json` 优先

`vision` 支持三种输出模式，默认行为取决于 stdout 是否为 TTY，显式开关覆盖默认：

| 场景 | 默认行为 | 输出 | 消费方 |
|------|----------|------|--------|
| stdout 是 TTY | 文本流式 | completion text 逐字打印 + 结尾 `[session_id: xxx]` | 终端人 |
| stdout 非 TTY | 原子文本 | 完整文本 + 结尾 session 提示 | 管道/捕获兜底 |
| `--json` | 原子 JSON | 单个 JSON 文档：成功 `{"text", "session_id", "parent_message_id"}`，失败 `{"error"}` | 脚本 / agent（推荐） |

- **开关**：`--stream` 强制流式、`--no-stream` 强制原子；与 `--json` 组合时 `--json` 恒为原子（`--json` 优先，`--stream` 与 `--json` 互斥，同时指定时报错）
- **TTY 检测**：`std::io::IsTerminal`（标准库，Rust 1.70+），无需新依赖
- **理由**：agent/脚本捕获 stdout 时是非 TTY，默认原子输出避免流式文本垃圾；TTY 检测是社区 CLI 惯例（gh / ripgrep / git 分页），显式开关覆盖默认保持可控
- **否决项**：不做流式 JSON（NDJSON 事件流）——`--json` 恒为原子，避免双路径复杂度与测试面；agent 实时进度需求由 SKILL 契约引导使用原子 `--json` 而非流式

### 决策 4：`vision` 参数与 stdin

```
visionary-server vision <image> [--prompt <q>] [--thinking] [--continue] [--session-id <id>] [--stream|--no-stream] [--json]
```

- `image` 位置参数：本地路径 / base64 / data URI（复用现有 `read_image` 逻辑，抽为共享函数）
- `image` 为 `-` 时从 stdin 读取全部字节
- `--continue` 与 `--session-id` 优先级与 MCP 一致（`session_id` 显式优先于 `continue_conversation`）
- `--stream` / `--no-stream`：显式覆盖 TTY 检测默认（见决策 3）
- 会话续聊解析逻辑从 `server.rs` handler 抽为 `pipeline` / `session` 层共享函数，两入口共用

### 决策 5：`login` / `logout` / `status` 语义

- `login`：复用 `login::run_login`（CDP 浏览器自动登录、阻塞等待），成功打印凭据摘要；已配置 token 时幂等提示。提示文案将"运行 `deepseek_vision_login`"泛化为"运行 `visionary-server login`"
- `logout`：复用 `login::run_logout`，打印清除结果
- `status`：复用 `deepseek_vision_status` 的检查逻辑（token 配置 + `probe_token` 真实探针），输出到 stdout；与 `doctor` 的差异在 scope——`status` 只查鉴权，`doctor` 查平台/浏览器/config 权限等完整环境。支持 `--json`：输出原子 JSON `{"authenticated", "token_configured", "smid_v2", "base_url", "token_valid"}`，供 agent 调用 `vision` 前预检；token 无效时无论是否 `--json` 均退出非零（见决策 6）

### 决策 6：退出码

所有子命令：成功退出 0；失败（未登录、图片读取失败、流水线错误、登录失败）退出非零并输出错误到 stderr。沿用 `doctor` 的 `std::process::exit(1)` 模式。

- `status` 的失败语义：token 未配置或探针失败视为"状态不佳"，退出非零（对齐 `gh auth status` 未认证退出非零、`doctor` token 无效退出非零）；`--json` 模式同样退出非零，但 stdout 仍输出完整状态 JSON（含 `token_valid: false`），调用方以退出码判断可用性、以 JSON 判断细节
- `vision` 的失败语义：`--json` 模式下所有失败（含未登录、图片读取失败、流水线错误）一律 stdout 输出 `{"error": ...}` 保持原子性；非 `--json` 模式输出错误文本到 stderr

### 决策 7：agent 调用契约（CLI + SKILL，内嵌二进制分发）

新增 `skills/visionary-cli/SKILL.md`（仓库内维护 + 编译进二进制），描述 CLI 的 agent 调用约定：

- `vision` 子命令用法与 `--json` 契约
- **调用规则：agent 调用 `vision` MUST 使用 `--json` 原子输出**——agent 以 spawn 子进程 + 捕获 stdout 的方式调用，流式文本无结构化边界（session_id 是结尾拼接文本），不可可靠解析；非 TTY 默认原子输出是第二重兑底
- `--json` 输出形状（成功/失败）、退出码约定
- 示例调用与解析方式

**分发方式（内嵌二进制）**：SKILL.md 通过 `include_str!` 编译进 `visionary-server` 二进制，新增 `skill install` 子命令将其写入 `~/.agents/skills/visionary-cli/SKILL.md`（`mkdir -p` 自动建目录）。

- **理由**：通过安装脚本（cargo-dist installer / brew / npm）装二进制的用户本地没有仓库，README 的 `cp -r skills/...` 无法执行（分发缺口）。内嵌保证 skill 与二进制同源、版本必然匹配，安装二进制即具备 skill，随时可重跑更新；不新增下载源、不动 cargo-dist 生成物（方案 A 定制 installer 模板撞并行 change 的 cargo-dist 迁移，风险高）
- **备选（已否决）**：
  - 仅文档不提供 skill——无调用约定的 agent 可能直接 `vision img.png` 收到流式文本而无法解析
  - 改 cargo-dist installer 模板随安装下载 SKILL.md——需定制 cargo-dist 生成物（侵入、与并行 change 冲突），且 skill 更新需重装；否决
  - README 给 `curl -o` 下载命令——版本漂移（main 可能超前于二进制），多步易错；否决
- **范围**：skill 是文档交付物，内嵌不引入新运行时依赖；`skill install` 仅写入标准 skill 目录，不触碰用户既有文件（已存在时覆盖并提示）

## Risks / Trade-offs

- **流式回调阻塞事件循环** → 单请求场景可接受；文档注明若未来支持并发需改 channel
- **`&mut dyn FnMut(&str)` 非 `Send`** → `#[tokio::main]` 默认多线程 runtime，但 `vision_completion` 由 `run_vision_pipeline` 直接 `.await`（非 `tokio::spawn`），`block_on` 不要求 future Send，当前可行；约束：流水线调用链保持同步 `.await`，若未来需 `tokio::spawn` 再改泛型 `F: FnMut(&str)` 或 channel
- **输出模式三态增加测试面**（TTY 流式 / 非 TTY 原子 / `--json` 原子）→ 输出模式决策收敛为纯函数（输入 TTY 布尔 + flags → 输出模式枚举），单测覆盖三态与开关组合
- **TTY 检测在测试环境不可靠**（CI 无 TTY）→ 测试通过 `--stream`/`--no-stream` 显式指定模式，不依赖 TTY 探测；TTY 默认值仅需少量集成验证
- **文案泛化遗漏**（login 超时提示仍引用 MCP 工具名）→ 实现时全局搜索 `deepseek_vision_login` / `deepseek_vision_logout` 文案，统一为 CLI 命令名
- **与并行 change 的 `server-cli` 能力重叠** → `cli-commands` 专注 4 个能力子命令，`server-cli` 专注入口/`doctor`/`init`；两 change 归档时合并 spec，无实现冲突（改动的文件有交集 `cli.rs`，若并行实现需协调，建议本 change 在其后或合并实现）

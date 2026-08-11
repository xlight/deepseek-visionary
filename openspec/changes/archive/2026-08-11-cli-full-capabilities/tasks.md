## 1. 核心层去 MCP 化重构

- [x] 1.1 将 `login.rs::run_login` / `run_logout` 返回类型从 `rmcp::model::CallToolResult` 改为 `Result<String>`（保留中文提示文本），更新 `server.rs` 的 `deepseek_vision_login` / `deepseek_vision_logout` handler 包装回 `CallToolResult`
- [x] 1.2 从 `server.rs::deepseek_vision` handler 抽出会话续聊解析逻辑（读 `SessionStore` + `session_id` 优先于 `continue_conversation`）为共享函数，MCP handler 与 CLI 共用
- [x] 1.3 抽出图片读取函数（本地路径 / base64 / data URI 检测与解码）到共享模块（`pipeline.rs` 或独立 util），`server.rs` 与 CLI 共用；补 `-`（stdin）读取分支

## 2. 流式输出

- [x] 2.1 `completion::vision_completion` 增加可选参数 `on_token: Option<F>`（泛型 `F: FnMut(&str) + Send`，因 rmcp `#[tool]` 要求 Send），`parse_sse` 解析到 content delta 时先回调再收集
- [x] 2.2 `pipeline::run_vision_pipeline` 增加对应回调参数并透传给 `vision_completion`（MCP 侧传 `None::<fn(&str)>`）
- [x] 2.3 为 `parse_sse` 流式分支补单元测试（复用现有 mock SSE 测试模式，验证回调逐块触发且收集结果一致）

## 3. CLI 子命令

- [x] 3.1 在 `cli.rs` 新增 `Command::Vision(VisionArgs)` / `Status` / `Login` / `Logout` 变体；`VisionArgs` 含 `image`（位置参数，`-` 表示 stdin）、`--prompt`、`--thinking`、`--continue`、`--session-id`、`--stream`、`--no-stream`、`--json`（冲突由 clap `conflicts_with` 拦截）
- [x] 3.2 实现输出模式决策纯函数：输入（stdout 是否 TTY + `--stream`/`--no-stream`/`--json` flags）→ 输出模式枚举（TTY 流式 / 原子文本 / 原子 JSON）；`--stream` 与 `--no-stream` 冲突、`--json` 与流式开关冲突时报错退出非零
- [x] 3.3 实现 `cmd_vision`：未登录时非 `--json` 模式 stderr 输出登录指引退出非零、`--json` 模式 stdout 输出 `{"error": ...}` 退出非零；按输出模式决策结果执行——流式打印 text + 结尾 `[session_id: xxx]` 提示，或原子文本，或 `--json` 原子输出 `{"text","session_id","parent_message_id"}` / `{"error"}`；图片读取失败/流水线错误时输出错误并退出非零
- [x] 3.4 实现 `cmd_status`：复用鉴权检查逻辑（token 配置 + smidV2 + Base URL + `probe_token` 真实探针）；默认可读文本输出到 stdout，`--json` 输出原子 JSON `{"authenticated","token_configured","smid_v2","base_url","token_valid"}`；token 未配置或探针失败时提示登录并退出非零（`--json` 模式 stdout 仍输出完整状态 JSON）
- [x] 3.5 实现 `cmd_login` / `cmd_logout`：复用 `login.rs` 重构后的函数（`run_login` 失败返回 `Err` 供 CLI 非零退出，MCP handler 映射回 `CallToolResult::error` 保持行为）；`doctor` 修复建议文案从 `deepseek_vision_login` 泛化为 `visionary-server login`
- [x] 3.6 实现 `Skill` 子命令（`skill install`）：`include_str!` 内嵌 `skills/visionary-cli/SKILL.md`，写入 `~/.agents/skills/visionary-cli/SKILL.md`（`mkdir -p` 自动建目录，已存在时覆盖并提示）；写入失败输出错误退出非零

## 4. 测试

- [x] 4.1 在 `cli.rs` 补 clap 解析测试：`vision` 各 flag 组合（`--prompt` / `--thinking` / `--continue` / `--session-id` / `--json`）、`--stream`/`--no-stream` 开关、`--stream --no-stream` 冲突、`--json --stream` 冲突、`vision -` stdin 形态、缺 `image` 参数报错、未知子命令报错
- [x] 4.2 为输出模式决策纯函数补单元测试：TTY+无开关→流式、非 TTY+无开关→原子文本、`--stream`/`--no-stream` 覆盖默认、`--json` 恒原子
- [x] 4.3 扩展 `tests/cli.rs` 集成测试：`status`（含 `--json` 有效/失效退出码与 JSON 形状）、`logout` 的端到端行为；`vision` 的未登录分支（用显式开关指定模式 + 隔离 HOME，不依赖 CI 的 TTY 探测，不触碰真实配置）
- [x] 4.4 补 `skill install` 测试：clap 解析（`skill install` 可解析、action 可省略）；集成测试验证隔离 HOME 下写入 `~/.agents/skills/visionary-cli/SKILL.md` 且内容与内嵌一致、重复执行覆盖仍退出 0、未知操作报错退出非零

## 5. 文档与 SKILL

- [x] 5.1 更新 README 的 CLI 工具表：补 `vision` / `status`（含 `--json`）/ `login` / `logout` 四行及 `vision` 参数说明（含 `--json` / `--stream` / `--no-stream` / TTY 默认 / stdin），并新增输出模式小节与示例
- [x] 5.2 在 `docs/` 补充 CLI 用法说明（`docs/cli.md`：命令示例、`--json` 输出形状、退出码约定、TTY 与开关行为、与 MCP 工具的能力对应关系）
- [x] 5.3 新增 `skills/visionary-cli/SKILL.md`（agent 调用契约，聚焦 vision 用法与 `--json` 契约，登录仅在错误恢复路径出现）；README 的安装说明改为 `visionary-server skill install`（skill 内嵌二进制，随安装具备）

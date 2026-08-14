# dsh-skill-cli-integration Tasks

## 1. CLI 解析（cli.rs）

- [x] 1.1 `InitArgs` 新增 `--dsh` 批量 flag（`/// Batch: also configure DeepSeek Harness.`），`visible_alias` 不需要（短名即 dsh）
- [x] 1.2 更新 `Init` 子命令 doc 注释与 `resolve_targets` 冲突错误文案、`from_name` 未知 agent 错误提示，agent 列表加 `dsh`
- [x] 1.3 单元测试：`init dsh` 位置参数解析、`--dsh` flag 解析、`init_flags_parse` 断言同步

## 2. skill 安装逻辑复用（cli.rs）

- [x] 2.1 抽出 `pub fn install_skill(dir: &Path) -> Result<PathBuf>`（创建目录 → 写内嵌 `EMBEDDED_SKILL` → 打印 已安装/已更新 提示 → 返回路径），`cmd_skill` 改用之（行为不变，仍写 `~/.agents/skills/visionary-cli/`）
- [x] 2.2 现有 `skill_subcommand_parses` 等测试保持通过

## 3. DSH agent 接入（onboarding.rs）

- [x] 3.1 `Agent` 枚举加 `DeepseekHarness`；`name()` → `"dsh"`；`from_name` 接受 `"dsh"` / `"deepseek-harness"` / `"deepseek_harness"` / `"harness"`；`all()` 数组加一员
- [x] 3.2 新增 `dsh_home_dir(home: &Path) -> PathBuf`（`DSH_HOME` 环境变量优先，回退 `home.join(".dsh")`）
- [x] 3.3 `detect_agents` 增加 DSH 分支：`find_in_path("dsh")` 或 `dsh_home_dir(home).join("profiles").exists()`；`config_path` 报告 DSH 根目录
- [x] 3.4 `resolve_targets` 增加 `--dsh` flag 收集；`write_agent` 增加 `DeepseekHarness` 分支 → `write_dsh`
- [x] 3.5 `write_dsh(home, dry_run)`：解析 `dsh_home_dir`；目标 1 `$DSH_HOME/skills/visionary-cli/`，目标 2 `~/.agents/skills/visionary-cli/`；dry-run 打印两路径，否则调用 `install_skill` 写两处并汇总打印
- [x] 3.6 onboarding 单元测试：DSH 检测（fake HOME 下 `~/.dsh/profiles/web` 存在 → 已检测；无则未检测）；`write_dsh` 写入两技能根且内容与内嵌一致；`--dry-run` 不落盘；`DSH_HOME` 环境变量覆盖（用环境锁串行化）

## 4. 文档

- [x] 4.1 新增 `docs/integrations/deepseek-harness.md`：一键接入（`init dsh`）、手动路径（二进制 + `skill install`）、DSH 技能发现根说明（`~/.dsh/skills` / `~/.agents/skills`）、DSH 中实际调用（模型经 bash 调 `visionary-server vision <image> --json`）、验证（`status --json`、重启 DSH 后技能目录出现 `visionary-cli`）、常见问题（未登录 / PATH / 技能根）、进阶：MCP 模式（`cordis.patch.yml` + `dsh-mcp-client` 手动配置示例）
- [x] 4.2 `README.md`：首段 agent 列表加 DeepSeek Harness；接入 MCP 宿主小节表格加一行（DeepSeek Harness | deepseek-harness.md | `init dsh`）；CLI + skill 章节注明 DSH 默认扫描 `~/.agents/skills`
- [x] 4.3 `docs/cli.md`：`init [agent]` 行说明补 DSH；agent 列表/链接处同步
- [x] 4.4 `docs/integrations/deepseek-harness.md` FAQ 补充 DSH 沙箱约束：`--continue` 续聊需写工作区外 session.json（沙箱下不持久，需 `danger-full-access` 或接受单次）、`login` 建议终端执行、`$DSH_AGENTS_HOME` 自定义时 `~/.dsh/skills` 兜底（openspec-design-review round 1）

## 5. 验证

- [x] 5.1 `cargo build -p visionary-server` 通过
- [x] 5.2 `cargo test -p visionary-server` 全部通过（含新增测试）
- [x] 5.3 `cargo test -p visionary-server --test cli` 二进制级回归通过
- [x] 5.4 `visionary-server init dsh --dry-run` 本地手测输出正确
- [x] 5.5 review 修订（openspec-design-review round 1）：agent-onboarding delta 检测条件文案对齐 `~/.dsh/profiles`；新增"DSH 接入文档说明沙箱与持久化约束"需求；design.md 增加沙箱 risk 条目

## 6. DSH 原生插件包 `packages/dsh-plugin/`（openspec-design-review round 2 新增，待实现）

- [x] 6.1 `packages/dsh-plugin/package.json`：`@xlight-oss/visionary-dsh`，`type: module`，`main`/`exports` → `lib/index.mjs`，`dsh: { bundle: { patch: "./cordis.patch.yml" } }`，peerDependencies `@deepseek-ai/cordis` / `@deepseek-ai/dsh-tools`，exports 含 `./cordis.patch.yml`，files 含 lib + cordis.patch.yml + README
- [x] 6.2 `packages/dsh-plugin/cordis.patch.yml`：`- insert:` 注册插件行（`id: visionary-vision`，`name: '@xlight-oss/visionary-dsh'`）
- [x] 6.3 `packages/dsh-plugin/lib/index.mjs`：导出 `name` / `inject: ["tools"]` / `Config`（schemastery：`binaryPath`、`loginTimeoutSeconds`、`visionTimeoutMs`）/ `apply(ctx, config)`；`ctx.tools.register(defineTool(...))` 注册 4 工具（`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`），execute 用 `child_process.spawn` 调 `visionary-server <cmd>`（vision/status 带 `--json`；login/logout 不带）并解析原子 JSON；`ctx.systemPrompt.section` 精简引导
- [x] 6.4 二进制解析：`Config.binaryPath` → `DEEPSEEK_VISIONARY_BIN` → PATH 查找；缺失/未登录返回含安装指引的错误
- [x] 6.5 `packages/dsh-plugin/README.md`：安装（`dsh plugin --profile web add @xlight-oss/visionary-dsh` 或本地路径）、配置、工具说明、与 skill + CLI / MCP 路径的关系
- [x] 6.6 本地验证：`dsh plugin --profile <test-profile> add <path>` 安装 → `--dump-config` 出现插件层 → 工具可被模型调用（识图冒烟）（隔离 headless profile 全链路：安装 → dump-config 出现 `@xlight-oss/visionary-dsh` 层 → 模型真实调用 `deepseek_vision` 识别红/白/蓝三色条带并返回 session_id；验证中修复两处：peer 依赖镜像 devDependencies（link 安装解析）、`resolveBinaryPath` 误用 `fs.promises.statSync`→改用 `statSync`）
- [x] 6.7 发布：npm publish `@xlight-oss/visionary-dsh`（独立于 cargo-dist；已定入 GitHub Actions release：新增 `.github/workflows/dsh-plugin-release.yml`，`v*` tag 触发，校验版本一致性后 `pnpm publish`，需配置 `NPM_TOKEN` secret；design.md Open Question 已更新）
- [x] 6.8 文档：`docs/integrations/deepseek-harness.md` 增插件安装为推荐路径；README 接入表格/安装章节补插件入口
- [x] 6.9 超时与中止传播：4 工具声明 `timeoutMs`（login = loginTimeoutSeconds×1000，loginTimeoutSeconds 默认读 `DEEPSEEK_LOGIN_TIMEOUT` env、未设置时 600；vision 默认 300000；status/logout 默认 60000）；`execute` 转发 `exec.signal` 至子进程（abort → kill）（openspec-design-review round 3/6）
- [x] 6.10 结果解析：以 stdout 原子 JSON 为准而非退出码（`status --json` 未登录非零退出仍解析 JSON 返回真实状态；`vision --json` 失败按退出码 + `{"error"}` 判定）（openspec-design-review round 3）
- [x] 6.11 系统提示并存引导：若装有 visionary-cli skill 优先原生工具（宿主级执行，续聊/登录不受 bash 沙箱限制）（openspec-design-review round 3）
- [x] 6.12 按子命令真实参数面调用：`login`/`logout` 不带 `--json`（直接透出文本）；`vision`/`status` 用 `--json` 原子解析（openspec-design-review round 4）
- [x] 6.13 选项传参一律等号形式 `--prompt=<v>` / `--session-id=<v>`（clap 对 `-` 开头空格值拒绝）（openspec-design-review round 4）
- [x] 6.14 `deepseek_vision` base64/data URI 入参解码写临时文件（`os.tmpdir()`）传路径，调用后清理；本地路径直传（Linux argv 单参上限 131072B）（openspec-design-review round 4）
- [x] 6.15 版本探测：apply 时 `visionary-server --version` 记录版本，与插件声明兼容版本不匹配时工具结果附警告（不阻断）（openspec-design-review round 5）
- [x] 6.16 文档 FAQ 安全提示：`image` 指向的文件会被读取并上传至 DeepSeek 服务，仅传有意分享的路径（openspec-design-review round 5）
- [x] 6.17 `.gitignore` 补 `node_modules/`（新增 packages/dsh-plugin 目录前）（openspec-design-review round 6）

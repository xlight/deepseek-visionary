## 1. CLI 子命令骨架（server-cli）

- [x] 1.1 在 `crates/visionary-server/Cargo.toml` 添加 `clap`（derive 特性）依赖
- [x] 1.2 重构 `main.rs`：引入 `Cli` 结构（`#[command]` + 可选 `command: Option<Subcommand>`），无参时走现有 serve 流程，抽出 `serve()` 函数
- [x] 1.3 实现 `--version` / `-V`：stdout 输出 `visionary-server <version>`，退出码 0
- [x] 1.4 实现 `doctor` 子命令：输出 config 路径与权限、浏览器检测（复用 `browser.rs` 发现逻辑）、凭据本地状态（复用 `AuthManager::validate()`）、平台/架构；严重失败退出非零
- [x] 1.5 从 `server.rs` 的 `deepseek_vision_status` 中提取真实 token 探针（`upload::create_pow_challenge` 调用）为共享函数（如 `auth::probe_token()`，带超时），status 工具与 doctor 共用；doctor 在探针失败时明确标注
- [x] 1.6 未知子命令：clap 默认输出 usage 到 stderr 并以非零退出，编写测试断言不进入 serve
- [x] 1.7 为无参路径添加回归保障：`cargo test` 断言 `Cli::parse()` 无参时 `command` 为 `None`，并确认 `mcp_probe.py smoke` 对无参二进制握手通过

## 2. init 引导子命令（agent-onboarding）

- [x] 2.1 实现 agent 检测：`command -v`/`where` 检测 `opencode` / `codex` / `claude` / `cursor` / Zed 扩展配置目录 / Claude Desktop 配置路径
- [x] 2.2 实现 `init` 无参交互模式：列出已检测 agent 与配置路径；无任何 agent 时输出提示并指向 `docs/integrations/`
- [x] 2.3 实现 `init <agent>` 定向模式：未安装时输出指引并退出非零
- [x] 2.4 实现 opencode 写入：顶层 `mcp` 键 + `type: "local"` + `command: ["visionary-server"]` + `timeout: 60000`（合并写 `~/.config/opencode/opencode.json`）
- [x] 2.5 实现 Codex 写入：优先 `codex mcp add deepseek-visionary -- visionary-server`（`--` 分隔符形式），兜底写 `~/.codex/config.toml` 的 `[mcp_servers.deepseek-visionary]`（注意键必须为 `mcp_servers`）
- [x] 2.6 实现 Claude Code 写入：优先 `claude mcp add --transport stdio deepseek-visionary -- visionary-server`（`--scope user` 非交互），失败兜底写用户级 `mcpServers`
- [x] 2.7 实现 Cursor 与 Claude Desktop 写入：`mcpServers` 形状（`.cursor/mcp.json` / 用户级配置 / `claude_desktop_config.json`）
- [x] 2.8 实现写前备份：`*.bak.<UTC时间戳>` 副本，备份失败即中止；实现严格 JSON 解析，解析失败即中止不覆盖
- [x] 2.9 实现 `--dry-run`：输出将写入的配置片段与目标路径，不落盘
- [x] 2.10 实现二进制 PATH 检测：`init` 前确认 `visionary-server` 在 PATH（或配置绝对路径），不在时提示安装方式并中止
- [x] 2.11 为 init 各写入路径编写单元测试（临时 HOME 目录 + 假 agent 命令），覆盖：opencode 形状、codex 键名、备份、dry-run、解析失败中止、二进制不在 PATH
- [x] 2.12 实现多 agent 批量 flags（`--opencode/--codex/--claude/--cursor/--claude-desktop`）与 `--yes` 免交互模式（对齐 mintlify/dgrep 范式）；位置参数与 flags 冲突时报错

## 3. 接入文档矩阵（agent-onboarding）

- [x] 3.1 新建 `docs/integrations/`，编写 `opencode.md`（含 timeout 60000 说明与冷启动坑）
- [x] 3.2 编写 `codex.md`（`mcp_servers` 键名提醒 + `codex mcp add` 两种路径）
- [x] 3.3 编写 `claude-code.md`（`claude mcp add` + 手动 `mcpServers`）
- [x] 3.4 编写 `claude-desktop.md` 与 `cursor.md`（`mcpServers` 手动配置）
- [x] 3.5 编写 `zed.md`（现有扩展安装路径，链接回 README）
- [x] 3.6 重构 README 安装章节：通用 MCP 接入优先（每 agent 一键命令），Zed 扩展降级为接入方式之一，链接 `docs/integrations/`

## 4. cargo-dist 分发（distribution）

- [x] 4.0 发布流程前置：将 workspace version bump 至与发布 tag 一致（如 0.2.0），确认 `tag == v{version}` 约定；此步为 cargo-dist 强制要求

- [x] 4.1 spike：`cargo dist init --hosting github` 生成 workflow，验证 `[[dist.extra-artifacts]]` 配置声明裸二进制与 mcpb（官方文档已确认 0.32 支持，实测确认字段行为）
  - 实测结论：0.32 的 extra-artifacts 是全局构建一次（is_global: true），`{target}`/`{exe}` 占位符不替换，会在发布时 MissingBinaries 报错；per-target 裸二进制/mcpb 改为 workflow 追加步骤
- [x] 4.2 配置 `[workspace.metadata.dist]`：installers = shell/powershell/npm/homebrew，cargo-dist 版本、tap、npm scope（实施时查 npm 注册表与 GitHub 可用性）
  - 已配：dist-workspace.toml 的 installers / cargo-dist-version / targets / github-runners / tap（`[dist]` 直接字段）/ npm scope（`[dist] npm-scope = "@xlight-oss"`）；
  - 外部依赖：需先创建 `xlight/homebrew-tap` 仓库、npm 组织 `xlight-oss` 与发布 token（`package: write`）、GitHub Actions secrets 配 `NPM_TOKEN`
- [x] 4.3 用 cargo-dist 生成 workflow 替换手写 `release.yml`，保留 tag 触发与 5 平台矩阵
  - 注意：`dist generate --mode ci` 会覆盖 release.yml，手工追加的裸二进制/mcpb/smoke 步骤需重新追加
- [x] 4.4 在生成的 workflow 中追加：各平台裸二进制上传（`visionary-server-<target-triple>`，Windows 带 `.exe`）
  - 实现：build-local-artifacts job 追加 "Package bare binary + MCPB" 步骤，产物经 upload-artifact 由 host job 一并上传 release
- [x] 4.5 在生成的 workflow 中追加：`scripts/build_mcpb.py` 构建并上传 `.mcpb`（保持 Registry 通道有效）
- [x] 4.6 发布后同步更新仓库根 `server.json`（MCP Registry 元数据）：version 与 5 平台 `fileSha256` 对齐本次发布，提交入库
  - 实现：新增 `scripts/update_server_json.py <version> <tag> <mcpb-dir>` 自动化更新；
  - 实际执行在 v0.2.0 发布后（任务 4.7 的发布清单含此步）
- [x] 4.7 打 v0.2.0 验证：5 平台裸二进制 + mcpb + archive + shell/powershell 安装器 + npm/homebrew 发布成功，Registry server.json 下载地址与 sha256 校验通过
  - 本地演练（已通过）：`dist build --artifacts=host --allow-dirty` 产出 installer.sh / npm pkg / homebrew.rb / tar.xz / sha256，URL 均指向 v0.2.0；裸二进制 + mcpb 构建脚本本地验证通过
  - 发布前置（需用户）：① 创建 `xlight/homebrew-tap` 仓库 ② npm scope `@xlight` 发布权限 ③ GitHub Actions secrets 配 `NPM_TOKEN` ④ 提交本 change 全部改动
  - 发布结果（2026-08-10）：run 31322222493 全绿，33 个 release 资产齐全（5 裸二进制 + 5 mcpb + 5 archive + sha256 + installer.sh/ps1 + npm pkg + homebrew.rb + source）；`mcp-publisher publish` 成功，Registry `latest` = 0.2.0 且 5 mcpb sha256 与 server.json 一致；npm/homebrew 发布未执行（publish-jobs 置空，见 4.2 外部依赖）
  - v0.2.1 发布结果（2026-08-10，npm/homebrew 通道打通）：run 31347310600 全绿，publish-npm + publish-homebrew-formula + announce 全部 ✓；
    - npm：`@xlight-oss/visionary-server@0.2.1`（npm-scope 配置键为 `[dist] npm-scope`，非 `[dist.installer.npm].scope`）；token 需 `package: write` 权限 + `bypass_2fa`（账号开 2FA 时）
    - homebrew：formula 已推 `xlight/homebrew-tap`（需先建仓库并初始化 main 分支，空仓库无 main ref 会 checkout 失败）；同版本重发需 commit 幂等（release.yml 已加 `git diff --cached --quiet` 跳过）
    - MCP Registry 0.2.1 已发布，server.json 同步提交（104d994）
    - v0.2.2 发布结果（2026-08-10，MCP instructions 改英文 + 版本 bump）：run 31352389388 全绿，npm/homebrew/Registry 三通道 0.2.2 全部确认（MCP instructions 已改为英文引导，工具 description 加主动调用触发）；
    - 经验：mcp-publisher 登录 token 有效期短（不足 1 天），每次 Registry 发布需重新设备流授权；可考虑后续迁 github-oidc CI 自动发布
- [x] 4.8 确认 Zed 扩展壳 `visionary-zed-ext` 无需改动即可下载裸二进制（方案 B 验收）
  - 代码确认：asset_name_for_platform 生成 `visionary-server-<arch>-<os>[.exe]` 与 workflow 追加的裸二进制命名一致，DownloadedFileType::Uncompressed 直接下载不依赖 archive
- [x] 4.9 验证 `cargo binstall visionary-server` 可用（cargo-dist URL schema 自动识别，如失败则补 binstall 元数据）
  - 实测结论：crate `publish = false`（不在 crates.io），binstall 无法自动解析包名，自动识别前提不成立；
  - 已补 `[package.metadata.binstall]`（pkg-url 指向 cargo-dist archive schema）；
  - v0.2.0 发布后实测（2026-08-10）：`cargo binstall visionary-server` 与 `--pkg-url` 模板均因 binstall 需先从 crates.io resolve 版本而失败（crate 不存在即 not found）；完整 binstall 支持需 `cargo publish`（crates.io API token）后再验

## 5. CI MCP smoke test（distribution）

- [x] 5.1 将 `scripts/mcp_probe.py` 改为双子命令 CLI：`smoke <binary-path>`（stdio initialize 握手 + 断言 4 工具，非零退出表示失败）与 `analyze <image-path> [prompt]`（保留现有测图逻辑）
- [x] 5.2 在 `ci.yml` 增加 PR 级 smoke test：host 平台构建产物跑 `mcp_probe.py smoke` + 断言 4 个工具
- [x] 5.3 在 release workflow 追加 per-target smoke test：每个目标平台的裸二进制跑 `mcp_probe.py smoke`

## 6. 收尾验证

- [x] 6.1 全量 `cargo test -p visionary-server` 通过，`cargo clippy` 无新增告警
- [x] 6.2 用 `mcp_probe.py smoke` 对本地构建验证无参 serve 握手与 4 工具
- [x] 6.3 端到端：`doctor` / `init opencode --dry-run` / `init opencode` 实机验证（含备份与恢复）
  - doctor：全部检查项 ✅ 退出 0；init opencode --dry-run：预览不落盘；init opencode：写前备份 .bak.<UTC> + 合并写入 + 保留既有条目
- [x] 6.4 README 与 `docs/integrations/` 全文走查：链接有效、命令可复制执行
  - README：通用 MCP 接入优先 + 文档矩阵 + apm 通道 + Zed 降级；6 份集成文档各含一键/手动/FAQ；命令与 onboarding.rs 实现一致

## 7. Zed 官方市场上架（marketplace）

- [x] 7.1 同步 `crates/visionary-zed-ext/extension.toml` 的 `version` 至 0.2.1，authors 补全邮箱（与 workspace 版本对齐）
- [x] 7.2 编写 `docs/zed-marketplace.md`：首次上架步骤、更新流程、检查清单、PR 要点
- [ ] 7.3 fork `zed-industries/extensions`，以 HTTPS submodule 挂入 `extensions/deepseek-visionary`（`path = "crates/visionary-zed-ext"`）
- [ ] 7.4 顶层 `extensions.toml` 登记条目并 `pnpm sort-extensions`，提交到 fork
- [ ] 7.5 创建 PR 至 `zed-industries/extensions`，通过 CI 与审核
- [x] 7.6 接入 `huacnlee/zed-extension-action@v2` 自动化：tag 触发自动更新 fork 的 submodule 指针与 `extensions.toml` version 并创建 PR（`.github/workflows/zed-extension-release.yml`，docs 补充说明）
  - 待启用：配置 `COMMITTER_TOKEN` secret（repo + workflow scopes）；首次上架 PR 合并后生效

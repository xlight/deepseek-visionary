//! CLI 入口：`--version` / `mcp-stdio` / `doctor` / `init` / `vision` / `status` /
//! `login` / `logout` / `skill` 子命令；无参数时输出 help 并退出（clap
//! `arg_required_else_help`，退出码 2）。
//!
//! 关键约束（design.md 决策 1）：MCP stdio 模式必须显式 `mcp-stdio` 子命令启动；
//! 所有 agent 配置都以 `command: ["visionary-cli", "mcp-stdio"]` 启动
//! （旧名 `visionary-server` 仍作为别名二进制产出，既有配置继续可用）。

use crate::hif::HifAuth;
use crate::pipeline::{self, VisionRequest};
use crate::server::VisionaryServer;
use crate::session::SessionStore;
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rmcp::ServiceExt;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

/// DeepSeek Visionary CLI (native binary).
///
/// 主命令名为 `visionary-cli`；`visionary-server` 仍作为别名二进制同时产出
/// （见 Cargo.toml 的两个 [[bin]]），让 v0.8.0 的既有安装与 agent 配置继续可用。
#[derive(Debug, Parser)]
#[command(
    name = "visionary-cli",
    version,
    about = "DeepSeek Visionary CLI",
    arg_required_else_help = true,
    after_help = "MCP stdio mode: run `visionary-cli mcp-stdio` to start the MCP server.\n\
                  Existing agent configs start the server without args; re-run `visionary-cli init` to migrate them."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the MCP stdio server (MCP mode entry point).
    McpStdio,
    /// Diagnose environment and config (browser, credentials, platform/architecture).
    Doctor,
    /// Bootstrap AI agent integration (opencode / codex / claude / cursor / claude-desktop / dsh).
    Init(InitArgs),
    /// Analyze an image with DeepSeek's vision model (CLI counterpart of deepseek_vision).
    Vision(VisionArgs),
    /// Extract text from an image via the OCR pipeline (CLI counterpart of deepseek_ocr).
    Ocr(OcrArgs),
    /// Lightweight auth status check (CLI counterpart of deepseek_vision_status).
    Status(StatusArgs),
    /// Browser auto-login (CLI counterpart of deepseek_vision_login).
    Login,
    /// Clear saved credentials (CLI counterpart of deepseek_vision_logout).
    Logout,
    /// Install the agent-calling-contract skill (embedded in the binary).
    Skill(SkillArgs),
    /// Manage the account pool (list / status / add / remove / use / migrate).
    Accounts(AccountsArgs),
}

/// `vision` 子命令参数：一张或多张图片 + 提示词/思考/会话续聊 + 输出模式开关。
#[derive(Debug, Args)]
pub struct VisionArgs {
    /// One or more images: local paths / base64 / data URI, or `-` to read stdin (single only).
    #[arg(required = true)]
    pub images: Vec<String>,
    /// Question about the image (default: detailed description in Chinese).
    #[arg(long, default_value = "请详细描述这张图片中的内容")]
    pub prompt: String,
    /// Enable DeepThink deep reasoning.
    #[arg(long)]
    pub thinking: bool,
    /// Continue the session for multi-image comparison and follow-up questions.
    #[arg(long)]
    pub continue_conversation: bool,
    /// Explicitly reuse a session_id (takes precedence over --continue-conversation).
    #[arg(long)]
    pub session_id: Option<String>,
    /// Force streaming output (overrides TTY detection).
    #[arg(long, conflicts_with = "no_stream")]
    pub stream: bool,
    /// Force atomic output (overrides TTY detection).
    #[arg(long, conflicts_with = "stream")]
    pub no_stream: bool,
    /// Atomic JSON output (disables streaming).
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,
    /// Upload pipeline: vision (default, full multimodal understanding) | ocr (text extraction).
    #[arg(long, value_enum, default_value = "vision")]
    pub model_type: ModelType,
}

/// modelType 合法值（clap ValueEnum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ModelType {
    /// 完整多模态图像理解（携带 x-model-type: vision）。
    Vision,
    /// OCR 文本提取（不携带 x-model-type，走服务端 OCR 管道）。
    Ocr,
}

impl ModelType {
    /// 与 `config::MODEL_TYPE_*` 常量对齐的字符串表示。
    pub fn as_str(self) -> &'static str {
        match self {
            ModelType::Vision => crate::config::MODEL_TYPE_VISION,
            ModelType::Ocr => crate::config::MODEL_TYPE_OCR,
        }
    }
}

/// `ocr` 子命令参数：与 `vision` 对齐，但不暴露 `--model-type`（恒为 ocr），
/// 默认 prompt 为文字提取语义。
#[derive(Debug, Args)]
pub struct OcrArgs {
    /// One or more images: local paths / base64 / data URI, or `-` to read stdin (single only).
    #[arg(required = true)]
    pub images: Vec<String>,
    /// Instruction for text extraction (default: output the text in the image verbatim).
    #[arg(long, default_value = "请原样输出图片中的文字内容")]
    pub prompt: String,
    /// Enable DeepThink deep reasoning.
    #[arg(long)]
    pub thinking: bool,
    /// Continue the session for follow-up questions.
    #[arg(long)]
    pub continue_conversation: bool,
    /// Explicitly reuse a session_id (takes precedence over --continue-conversation).
    #[arg(long)]
    pub session_id: Option<String>,
    /// Force streaming output (overrides TTY detection).
    #[arg(long, conflicts_with = "no_stream")]
    pub stream: bool,
    /// Force atomic output (overrides TTY detection).
    #[arg(long, conflicts_with = "stream")]
    pub no_stream: bool,
    /// Atomic JSON output (disables streaming).
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,
}

impl From<OcrArgs> for VisionArgs {
    fn from(a: OcrArgs) -> Self {
        VisionArgs {
            images: a.images,
            prompt: a.prompt,
            thinking: a.thinking,
            continue_conversation: a.continue_conversation,
            session_id: a.session_id,
            stream: a.stream,
            no_stream: a.no_stream,
            json: a.json,
            model_type: ModelType::Ocr,
        }
    }
}

/// `status` 子命令参数。
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Atomic JSON output.
    #[arg(long)]
    pub json: bool,
}

/// `accounts` 子命令组参数。
#[derive(Debug, Args)]
pub struct AccountsArgs {
    #[command(subcommand)]
    pub action: AccountsAction,
}

/// `accounts` 子命令组。
#[derive(Debug, Subcommand)]
pub enum AccountsAction {
    /// List accounts in the pool (pure read, no network requests).
    List(AccountsJsonArgs),
    /// Probe every account concurrently and report health.
    Status(AccountsJsonArgs),
    /// Add an account through browser login (bypasses the "token already configured" guard).
    Add(AccountsJsonArgs),
    /// Remove an account from the pool by id.
    Remove(AccountsRemoveArgs),
    /// Pin an account (or unpin with --clear).
    Use(AccountsUseArgs),
    /// Import the existing single-account credentials into the pool (one-off, idempotent).
    Migrate(AccountsJsonArgs),
}

/// `accounts` 子命令的通用参数。
#[derive(Debug, Args)]
pub struct AccountsJsonArgs {
    /// Atomic JSON output (same shape as PoolView).
    #[arg(long)]
    pub json: bool,
}

/// `accounts remove` 参数。
#[derive(Debug, Args)]
pub struct AccountsRemoveArgs {
    /// Account id (as shown by `accounts list`).
    pub id: String,
    /// Atomic JSON output.
    #[arg(long)]
    pub json: bool,
}

/// `accounts use` 参数。
#[derive(Debug, Args)]
pub struct AccountsUseArgs {
    /// Account id to pin (omit together with --clear to unpin).
    pub id: Option<String>,
    /// Clear the pinned account (rotation resumes).
    #[arg(long, conflicts_with = "id")]
    pub clear: bool,
    /// Atomic JSON output.
    #[arg(long)]
    pub json: bool,
}

/// `skill` 子命令参数。
#[derive(Debug, Args)]
pub struct SkillArgs {
    /// Sub-action: install (currently the only one).
    pub action: Option<String>,
}

/// `init` 子命令参数：位置参数（单个 agent）或多选 flags（批量）+ `--yes` / `--dry-run`。
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target agent name: opencode / codex / claude / claude-desktop / cursor / dsh.
    pub agent: Option<String>,
    /// Batch: also configure opencode.
    #[arg(long)]
    pub opencode: bool,
    /// Batch: also configure Codex.
    #[arg(long)]
    pub codex: bool,
    /// Batch: also configure Claude Code.
    #[arg(long)]
    pub claude: bool,
    /// Batch: also configure Claude Desktop.
    #[arg(long, visible_alias = "claude-desktop")]
    pub claude_desktop: bool,
    /// Batch: also configure Cursor.
    #[arg(long)]
    pub cursor: bool,
    /// Batch: also configure DeepSeek Harness (dsh, skill + CLI route).
    #[arg(long)]
    pub dsh: bool,
    /// Non-interactive: skip confirmation and write directly.
    #[arg(long)]
    pub yes: bool,
    /// Only preview the config to be written, without touching disk.
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::McpStdio) => serve().await,
        Some(Command::Doctor) => cmd_doctor().await,
        Some(Command::Init(args)) => crate::onboarding::cmd_init(args),
        Some(Command::Vision(args)) => cmd_vision(args, ModelType::Vision).await,
        Some(Command::Ocr(args)) => cmd_vision(args.into(), ModelType::Ocr).await,
        Some(Command::Status(args)) => cmd_status(args).await,
        Some(Command::Login) => cmd_login().await,
        Some(Command::Logout) => cmd_logout().await,
        Some(Command::Skill(args)) => cmd_skill(args),
        Some(Command::Accounts(args)) => cmd_accounts(args).await,
        // 无参数由 clap 的 arg_required_else_help 拦截（输出 help 并 exit 2），不可达
        None => unreachable!("no-args invocation is rejected by arg_required_else_help"),
    }
}

/// MCP stdio serve 模式（`mcp-stdio` 子命令入口，行为与引入 CLI 前完全一致）。
async fn serve() -> Result<()> {
    tracing::info!(
        "starting DeepSeek Visionary MCP server v{}",
        env!("CARGO_PKG_VERSION")
    );

    let config = crate::config::Config::load()?;
    let service = VisionaryServer::new(config);
    let running = service.serve(rmcp::transport::stdio()).await?;
    // 阻塞等待服务运行直到连接结束（对应官方 calculator_stdio 示例的 waiting()）
    running.waiting().await?;
    Ok(())
}

/// `doctor`：逐项诊断环境与配置（对齐 ironclaw 范式：每项 ✓/✗/⚠ + 修复建议）。
///
/// 严重失败（无浏览器 或 token 无效）时退出非零。
async fn cmd_doctor() -> Result<()> {
    let mut lines = Vec::new();
    let mut issues = 0usize;

    lines.push(format!("DeepSeek Visionary v{}", env!("CARGO_PKG_VERSION")));
    lines.push(String::new());

    // 平台/架构
    lines.push(format!(
        "- Platform: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    // 配置文件路径与权限
    match crate::config::config_file() {
        Ok(path) => {
            let exists = path.exists();
            let perms = if exists {
                config_file_perms(&path)
            } else {
                "not created".to_string()
            };
            lines.push(format!("- Config: {} ({})", path.display(), perms));
        }
        Err(e) => {
            lines.push(format!("- Config: [FAIL] {e}"));
            issues += 1;
        }
    }

    // 浏览器检测
    match crate::browser::find_browser() {
        Ok(b) => lines.push(format!("- Browser: [OK] {}", b.display())),
        Err(e) => {
            lines.push(format!("- Browser: [FAIL] {e}"));
            issues += 1;
        }
    }

    // 进程完整性级别：低于 Medium 时派生出的浏览器会立刻崩溃（0x80000003）。
    match crate::integrity::current_level() {
        Some(level) if level.is_below_medium() => {
            lines.push(format!(
                "- Integrity: [FAIL] {} — 浏览器会以相同低完整性启动并立即退出（0x80000003）。修复：icacls \"{exe}\" /setintegritylevel M",
                level.name(),
                exe = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "<当前可执行文件>".to_string()),
            ));
            issues += 1;
        }
        Some(level) => lines.push(format!("- Integrity: [OK] {}", level.name())),
        None => {}
    }

    // 凭据：本地检查 + 真实 token 探针
    let config = crate::config::Config::load()?;
    let pool_mode = config.pool_mode();
    let pool = config.pool();
    let authenticated = if pool_mode {
        pool.usable_count() > 0
    } else {
        config.is_authenticated()
    };
    lines.push(format!(
        "- Authenticated: {}",
        if authenticated { "[OK]" } else { "[FAIL]" }
    ));

    if pool_mode {
        // 池模式 doctor 只报告**池概况与当前使用账号**：逐账号联网探测是
        // `accounts status` 的职责（doctor 是排障入口，批量探针会拖慢诊断）。
        lines.push(format!(
            "- Account pool: {} account(s), {} usable",
            pool.accounts().len(),
            pool.usable_count()
        ));
        // 在克隆的池上试选：不推进游标、不写盘。
        let mut probe_pool = pool.clone();
        match probe_pool.next_usable() {
            crate::accounts::AccountSelection::Selected(a) => {
                let label = if a.label.is_empty() {
                    a.account_id.clone()
                } else {
                    a.label.clone()
                };
                lines.push(format!("- Current account: {label}"));
            }
            crate::accounts::AccountSelection::PinnedUnavailable { label, reason } => {
                lines.push(format!(
                    "- Current account: {label} [WARN] {}",
                    reason.message()
                ));
            }
            crate::accounts::AccountSelection::None => {
                lines.push("- Current account: none usable".into());
            }
        }
        let ignored = ignored_env_vars(&config);
        if !ignored.is_empty() {
            lines.push(format!(
                "- Environment ignored (pool mode): {}",
                ignored.join(", ")
            ));
        }
    }

    // 池模式下只探「当前凭据」（兼容配置）对应的那一个账号，不逐账号探测。
    if config.is_authenticated() {
        // 只读探针（GET /api/v0/users/current）：不发起上传、不求解 PoW、不消耗配额。
        match crate::account_health::probe_config(&config, Duration::from_secs(15)).await {
            Ok(health) => {
                lines.push(format!(
                    "- Token validation: [OK] (read-only probe passed at {}, account {})",
                    config.base_url,
                    if health.label.is_empty() {
                        &health.account_id
                    } else {
                        &health.label
                    }
                ));
                if health.muted {
                    let until = health
                        .mute_until_utc()
                        .unwrap_or_else(|| "unknown".to_string());
                    lines.push(format!(
                        "- Account muted: [WARN] uploads are muted until {until}"
                    ));
                }
            }
            Err(e) => {
                lines.push(format!("- Token validation: [FAIL] probe failed: {e}"));
                issues += 1;
            }
        }
    }

    if issues > 0 {
        lines.push(String::new());
        if pool_mode {
            // 池模式下环境变量被忽略，提示不得引导用户去设置它。
            lines.push(
                "To fix: run `visionary-cli accounts list` to inspect the pool, or `visionary-cli accounts add` to add an account."
                    .into(),
            );
        } else {
            lines.push(
                "To fix: run `visionary-cli login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable."
                    .into(),
            );
        }
        lines.push("Install Chrome / Chromium / Edge for auto-login support.".into());
    }

    for line in &lines {
        println!("{line}");
    }

    if issues > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// 账号池模式下**被忽略**的环境变量。
///
/// 池存在时 env 不参与选号（也不登记为账号）：用户以为指定了它、实际多数请求用的
/// 是别的号，比忽略更容易误导——因此 SHALL 在 status / accounts list 里明确提示。
pub(crate) fn ignored_env_vars(config: &crate::config::Config) -> Vec<String> {
    if !config.pool_mode() {
        return Vec::new();
    }
    [
        "DEEPSEEK_USER_TOKEN",
        "DEEPSEEK_SMIDV2",
        "DEEPSEEK_CF_CLEARANCE",
    ]
    .iter()
    .filter(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
    .map(|k| k.to_string())
    .collect()
}

/// 配置文件权限的可读描述（Unix 下显示八进制 mode）。
fn config_file_perms(path: &PathBuf) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| format!("mode {:o}", m.permissions().mode() & 0o777))
            .unwrap_or_else(|_| "unreadable".to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        "n/a".to_string()
    }
}

/// `vision` 子命令的输出模式（design 决策 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    /// 文本流式：completion 增量逐块打印（stdout 为 TTY 或 `--stream`）。
    Stream,
    /// 原子文本：完整结果一次性输出（stdout 非 TTY 或 `--no-stream`）。
    AtomicText,
    /// 原子 JSON：`--json`，供脚本/agent 消费。
    Json,
}

/// 输出模式决策纯函数：输入（stdout 是否 TTY + 三个开关）→ 输出模式。
///
/// 冲突（`--stream` 与 `--no-stream`、`--json` 与 `--stream`）由 clap `conflicts_with`
/// 在解析期拦截，此处仍防御性检查以保证纯函数可独立测试。
fn resolve_output_mode(
    tty: bool,
    stream: bool,
    no_stream: bool,
    json: bool,
) -> Result<OutputMode, String> {
    if stream && no_stream {
        return Err("--stream and --no-stream cannot be used together".into());
    }
    if json && stream {
        return Err("--json and --stream cannot be used together (--json is always atomic)".into());
    }
    if json {
        return Ok(OutputMode::Json);
    }
    if stream {
        return Ok(OutputMode::Stream);
    }
    if no_stream {
        return Ok(OutputMode::AtomicText);
    }
    Ok(if tty {
        OutputMode::Stream
    } else {
        OutputMode::AtomicText
    })
}

/// `vision`：CLI 方式运行完整 vision 流水线（design 决策 3/4/6）。
async fn cmd_vision(args: VisionArgs, forced: ModelType) -> Result<()> {
    let mode = match resolve_output_mode(
        std::io::stdout().is_terminal(),
        args.stream,
        args.no_stream,
        args.json,
    ) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let config = crate::config::Config::load()?;
    // 准入判定：账号池模式以「池中有无可用账号」为准（兼容配置可为空）；
    // 单账号模式维持既有 token 判定。
    if config.pool_mode() {
        if config.pool().usable_count() == 0 {
            fail(
                mode,
                "No usable account in the pool: run `visionary-cli accounts list` to inspect the pool, or `visionary-cli accounts add` to add an account.",
            );
        }
    } else if !config.is_authenticated() {
        fail(
            mode,
            "Not logged in: run `visionary-cli login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable.",
        );
    }

    // modelType 优先级：子命令强制（ocr）> 命令行 --model-type > 配置默认。
    // `ocr` 子命令恒为 Ocr；`vision` 子命令用 --model-type 或配置。
    let model_type = if forced == ModelType::Ocr || args.model_type == ModelType::Ocr {
        Some(ModelType::Ocr.as_str().to_string())
    } else {
        config.model_type.clone()
    };

    // 读取图片（路径 / base64 / data URI / stdin；stdin 仅限单图）
    if args.images.len() > 1 && args.images.iter().any(|i| i == "-") {
        fail(mode, "stdin (`-`) can only be used with a single image");
    }
    let mut images_data = Vec::with_capacity(args.images.len());
    for image in &args.images {
        match pipeline::read_image(image) {
            Ok(d) => images_data.push(d),
            Err(e) => fail(mode, &e),
        }
    }

    // 会话连续性：由流水线在**选号之后**统一解析（会话绑定账号，换号即断链），
    // 调用方不再预填 session_id / parent_message_id。
    let session_store = SessionStore::with_legacy_owner(pipeline::legacy_session_owner(&config));

    let request = VisionRequest {
        images_data,
        prompt: args.prompt,
        thinking: args.thinking,
        session_id: args.session_id.clone(),
        parent_message_id: None,
        model_type,
    };
    let hif = HifAuth::new(config.clone());

    match mode {
        OutputMode::Stream => {
            // 流式：回调把 completion 增量逐块写到 stdout。
            // 内层块作用域让闭包与 stdout 借用自然结束，避免 drop()。
            let result = {
                let mut stdout = std::io::stdout();
                let mut on_token = |tok: &str| {
                    let _ = write!(stdout, "{tok}");
                    let _ = stdout.flush();
                };
                pipeline::run_vision_pipeline(
                    &config,
                    &hif,
                    &session_store,
                    request,
                    args.continue_conversation,
                    Some(&mut on_token),
                )
                .await
            };
            match result {
                Ok(output) => {
                    println!(
                        "\n---\n[session_id: {}] (use --continue-conversation to keep chatting)",
                        output.session_id
                    );
                }
                Err(e) => fail(mode, &format!("Vision analysis failed: {e:#}")),
            }
        }
        OutputMode::AtomicText | OutputMode::Json => {
            // 原子模式：不流式（None::<fn(&str)> 满足 Send 约束）
            match pipeline::run_vision_pipeline::<fn(&str)>(
                &config,
                &hif,
                &session_store,
                request,
                args.continue_conversation,
                None,
            )
            .await
            {
                Ok(output) => {
                    if mode == OutputMode::Json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "text": output.text,
                                "session_id": output.session_id,
                                "parent_message_id": output.parent_message_id,
                            })
                        );
                    } else {
                        println!("{}", output.text);
                        println!(
                            "\n---\n[session_id: {}] (use --continue-conversation to keep chatting)",
                            output.session_id
                        );
                    }
                }
                Err(e) => fail(mode, &format!("Vision analysis failed: {e:#}")),
            }
        }
    }
    Ok(())
}

/// `status`：轻量鉴权检查（design 决策 5）。token 未配置或探针失败时退出非零。
async fn cmd_status(args: StatusArgs) -> Result<()> {
    let config = crate::config::Config::load()?;
    let pool_mode = config.pool_mode();
    let pool = config.pool();
    let usable = pool.usable_count();
    // 凭据字段的口径：池模式描述**当前使用账号**（固定 > 最近使用 > 无）；
    // 单账号模式描述兼容配置（config.json / 环境变量）。
    let current_id = pool.current_account_id();
    let creds = match current_id.as_deref() {
        Some(id) => config.credentials_for(id),
        None => config.credentials(),
    };
    // 池模式：「已登录」= 池中有可用账号（兼容配置可为空）；单账号模式 = token 已配置。
    let authenticated = if pool_mode {
        usable > 0
    } else {
        config.is_authenticated()
    };
    // token_valid 反映**当前使用账号**的探针结果（只读 GET /api/v0/users/current：
    // 与 doctor / MCP 状态工具同一判定语义，纯 GET、无 PoW、无配额消耗）。
    let token_valid = match current_id.as_deref() {
        Some(id) => crate::account_health::probe_account_id(&config, id, Duration::from_secs(15))
            .await
            .is_ok(),
        None => {
            if config.is_authenticated() {
                crate::account_health::probe_config(&config, Duration::from_secs(15))
                    .await
                    .is_ok()
            } else {
                false
            }
        }
    };
    // 退出判据：池模式以「池中有无可用账号」为准——兼容配置为空或探针失败，
    // 但池里有可用账号时 SHALL NOT 退出非零。
    let ok = if pool_mode { usable > 0 } else { token_valid };
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "authenticated": authenticated,
                "token_configured": !creds.user_token.is_empty(),
                "smid_v2": !creds.smid_v2.is_empty(),
                "base_url": config.base_url,
                "token_valid": token_valid,
                // 账号池概况作为**新增子对象**呈现（既有顶层字段保持不变）。
                "pool": {
                    "enabled": pool_mode,
                    "total": pool_view(&config).total,
                    "healthy": pool_view(&config).healthy,
                    "muted": pool_view(&config).muted,
                    "expired": pool_view(&config).expired,
                    "current": pool_view(&config).current,
                    "env_ignored": !ignored_env_vars(&config).is_empty(),
                },
            })
        );
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut lines = vec![
        format!("DeepSeek Vision v{}", env!("CARGO_PKG_VERSION")),
        String::new(),
        format!(
            "- Authenticated: {}",
            if authenticated { "[OK]" } else { "[FAIL]" }
        ),
        format!(
            "- Token configured: {}",
            if creds.user_token.is_empty() {
                "No"
            } else {
                "Yes"
            }
        ),
        format!(
            "- smidV2 cookie: {}",
            if creds.smid_v2.is_empty() {
                "[FAIL] (optional)"
            } else {
                "[OK]"
            }
        ),
        format!("- Base URL: {}", config.base_url),
    ];
    if pool_mode {
        lines.push(format!(
            "- Account pool: {} account(s), {} usable",
            pool.accounts().len(),
            usable
        ));
        let ignored = ignored_env_vars(&config);
        if !ignored.is_empty() {
            lines.push(format!(
                "- Environment ignored (pool mode): {}",
                ignored.join(", ")
            ));
        }
    }
    if authenticated {
        lines.push(format!(
            "- Token validation: {}",
            if token_valid {
                "[OK] (live probe passed)"
            } else {
                "[FAIL] probe failed"
            }
        ));
    }
    if !ok {
        lines.push(String::new());
        if pool_mode {
            // 池模式：环境变量被忽略，SHALL NOT 引导用户去设置 DEEPSEEK_USER_TOKEN。
            lines.push(
                "No usable account in the pool: run `visionary-server accounts list` to inspect the pool, or `visionary-server accounts add` to add an account."
                    .into(),
            );
        } else {
            lines.push(
                "Not logged in: run `visionary-server login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable."
                    .into(),
            );
        }
    }
    for line in &lines {
        println!("{line}");
    }
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

/// `login`：浏览器自动登录（复用 login.rs 流程，失败退出非零）。
async fn cmd_login() -> Result<()> {
    let config = crate::config::Config::load()?;
    match crate::login::run_login(&config).await {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `logout`：清除保存的凭据。
async fn cmd_logout() -> Result<()> {
    let config = crate::config::Config::load()?;
    let text = crate::login::run_logout(&config).await?;
    println!("{text}");
    Ok(())
}

/// 内嵌的 agent 调用契约 SKILL.md（随二进制分发，版本必然匹配）。
const EMBEDDED_SKILL: &str = include_str!("../../../skills/visionary-cli/SKILL.md");

/// 把内嵌 SKILL.md 安装到指定技能根目录（创建 `<dir>/SKILL.md`，目录自动创建，
/// 已存在则覆盖并提示"已更新"）。返回写入的文件路径。
///
/// 供 `skill install`（`~/.agents/skills/visionary-cli/`）与 `init dsh`
/// （DSH 技能根 `$DSH_HOME/skills/` 与 `~/.agents/skills/`）复用。
pub fn install_skill(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir).context("create skill directory")?;
    let path = dir.join("SKILL.md");
    let existed = path.exists();
    std::fs::write(&path, EMBEDDED_SKILL).context("write SKILL.md")?;
    if existed {
        println!("Skill updated: {}", path.display());
    } else {
        println!("Skill installed: {}", path.display());
    }
    Ok(path)
}

/// `skill install`：把内嵌 SKILL.md 写入 `~/.agents/skills/visionary-cli/SKILL.md`。
///
/// design 决策 7：通过安装脚本（cargo-dist / brew / npm）装二进制的用户本地没有仓库，
/// 无法 `cp -r skills/...`；内嵌保证安装二进制即具备 skill，已存在时覆盖并提示。
/// DSH（DeepSeek Harness）默认扫描 `~/.agents/skills`，因此该位置同时覆盖 DSH；
/// `init dsh` 会额外写入 DSH 专属技能根（见 onboarding.rs）。
fn cmd_skill(args: SkillArgs) -> Result<()> {
    match args.action.as_deref() {
        Some("install") | None => {}
        Some(other) => {
            eprintln!("Unknown skill action: {other} (only `install` is supported)");
            std::process::exit(1);
        }
    }

    // ~/.agents/skills/visionary-cli/（镜像数据目录逻辑，但固定到 ~/.agents）
    // 用户目录走 onboarding::home_dir()（Unix: HOME，Windows: USERPROFILE），
    // 避免 Windows 无 HOME 时错误回退到当前工作目录。
    let home = crate::onboarding::home_dir()?;
    let dir = home.join(".agents").join("skills").join("visionary-cli");
    install_skill(&dir)?;
    println!(
        "Tip: if your agent's default skills dir is not ~/.agents/skills (e.g. Claude Code uses ~/.claude/skills),\n\
         move the visionary-cli directory to that agent's default dir, e.g.:\n\
         mv {} ~/.claude/skills/",
        dir.display()
    );
    Ok(())
}

/// `accounts` 子命令组入口。
async fn cmd_accounts(args: AccountsArgs) -> Result<()> {
    match args.action {
        AccountsAction::List(a) => cmd_accounts_list(a),
        AccountsAction::Status(a) => cmd_accounts_status(a).await,
        AccountsAction::Add(a) => cmd_accounts_add(a).await,
        AccountsAction::Remove(a) => cmd_accounts_remove(a),
        AccountsAction::Use(a) => cmd_accounts_use(a),
        AccountsAction::Migrate(a) => cmd_accounts_migrate(a).await,
    }
}

/// 重新读取磁盘上的账号池并同步到内存（写操作后调用）。
///
/// 账号池的**权威**是磁盘上的 accounts.json；CLI 进程内的 Config 是启动时的快照——
/// 写操作（remove / use / migrate / add）后必须重载，否则同一进程里紧接着输出的
/// 视图仍是旧数据。
fn reload_pool(config: &crate::config::Config) {
    if let Ok(pool) = crate::accounts::load_pool() {
        if let Ok(mut guard) = config.pool.write() {
            *guard = pool;
        }
    }
}

/// 池视图（三端同源口径）+ 环境变量忽略标记。
fn pool_view(config: &crate::config::Config) -> crate::accounts::PoolView {
    let pool = config.pool();
    let env_ignored = !ignored_env_vars(config).is_empty();
    pool.view(env_ignored)
}

/// 池未启用时的提示（区分「有兼容凭据 → 可迁移」与「完全未配置」）。
fn pool_disabled_notice(config: &crate::config::Config) -> String {
    if config.is_authenticated() {
        "The account pool is not enabled yet, but config.json still holds credentials.\n  Run `visionary-cli accounts migrate` to import it into the pool (idempotent, non-destructive)."
            .to_string()
    } else {
        "The account pool is not enabled. Run `visionary-cli accounts add` to add the first account."
            .to_string()
    }
}

/// 状态标签（healthy / muted / expired）。
fn status_label(status: crate::accounts::AccountStatus) -> &'static str {
    use crate::accounts::AccountStatus;
    match status {
        AccountStatus::Healthy => "healthy",
        AccountStatus::Muted => "muted",
        AccountStatus::Expired => "expired",
    }
}

/// 环境变量被忽略的提示（池模式下 env 不参与选号）。
fn env_ignored_notice(config: &crate::config::Config) -> Option<String> {
    let ignored = ignored_env_vars(config);
    if ignored.is_empty() {
        return None;
    }
    Some(format!(
        "- Environment ignored (pool mode): {} — rotation uses the account pool only.",
        ignored.join(", ")
    ))
}

/// `accounts list`：纯读（**不发起任何网络请求**）。
fn cmd_accounts_list(args: AccountsJsonArgs) -> Result<()> {
    let config = crate::config::Config::load()?;
    let view = pool_view(&config);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&view)?);
        return Ok(());
    }

    let mut lines = vec![
        format!("DeepSeek Vision v{}", env!("CARGO_PKG_VERSION")),
        String::new(),
    ];
    if !view.enabled {
        lines.push("- Account pool: not enabled".to_string());
        lines.push(pool_disabled_notice(&config));
    } else {
        lines.push(format!(
            "- Account pool: {} account(s) — healthy {}, muted {}, expired {}",
            view.total, view.healthy, view.muted, view.expired
        ));
        match &view.current {
            Some(id) => {
                let label = view
                    .accounts
                    .iter()
                    .find(|a| &a.id == id)
                    .map(|a| a.label.clone())
                    .unwrap_or_else(|| id.clone());
                lines.push(format!("- Current account: {label} ({id})"));
            }
            None => {
                lines.push("- Current account: none (no account has been used yet)".to_string())
            }
        }
        if let Some(pinned) = &view.pinned {
            lines.push(format!(
                "- Pinned account: {pinned} (rotation is suspended for it)"
            ));
        }
        lines.push(String::new());
        for account in &view.accounts {
            let mut line = format!(
                "  [{}] {} ({})",
                status_label(account.status),
                account.label,
                account.id
            );
            if let Some(until) = account.mute_until {
                line.push_str(&format!(
                    " — muted until {}",
                    crate::client::format_epoch_utc(until as i64)
                ));
            }
            if let Some(last) = &account.last_used {
                line.push_str(&format!(" — last used {last}"));
            }
            if account.pinned {
                line.push_str(" — pinned");
            }
            lines.push(line);
        }
    }
    if let Some(notice) = env_ignored_notice(&config) {
        lines.push(notice);
    }
    for line in &lines {
        println!("{line}");
    }
    Ok(())
}

/// `accounts status`：对池中每个账号并发发一次只读探针后输出。
async fn cmd_accounts_status(args: AccountsJsonArgs) -> Result<()> {
    let config = crate::config::Config::load()?;
    if !config.pool_mode() {
        let view = pool_view(&config);
        if args.json {
            println!("{}", serde_json::to_string_pretty(&view)?);
            return Ok(());
        }
        println!("DeepSeek Vision v{}", env!("CARGO_PKG_VERSION"));
        println!();
        println!("- Account pool: not enabled");
        println!("{}", pool_disabled_notice(&config));
        return Ok(());
    }

    // 并发只读探针：整体耗时 ≈ 单次探针（预算见 DEEPSEEK_ACCOUNTS_TIMEOUT）。
    let outcomes =
        crate::account_health::refresh_pool(&config, crate::account_health::accounts_budget())
            .await;
    // 结论落盘（合并写回：不会覆盖其它进程的并发变更）。
    let path = crate::accounts::pool_file()?;
    crate::accounts::merge_and_save(&path, |disk| {
        crate::account_health::apply_probe_outcomes(disk, &outcomes);
        Ok(())
    })?;
    // 同步内存态（本进程后续的视图与选号立即可见）。
    if let Ok(mut pool) = config.pool.write() {
        crate::account_health::apply_probe_outcomes(&mut pool, &outcomes);
    }

    let view = pool_view(&config);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&view)?);
        return Ok(());
    }

    let mut lines = vec![
        format!("DeepSeek Vision v{}", env!("CARGO_PKG_VERSION")),
        String::new(),
        format!(
            "- Account pool: {} account(s) — healthy {}, muted {}, expired {}",
            view.total, view.healthy, view.muted, view.expired
        ),
    ];
    for (id, outcome) in &outcomes {
        let detail = match outcome {
            crate::account_health::ProbeOutcome::Healthy => "healthy".to_string(),
            crate::account_health::ProbeOutcome::Muted { until } => match until {
                Some(ts) => format!(
                    "muted until {}",
                    crate::client::format_epoch_utc(*ts as i64)
                ),
                None => "muted (unknown deadline)".to_string(),
            },
            crate::account_health::ProbeOutcome::Expired => "expired".to_string(),
            crate::account_health::ProbeOutcome::Failed(reason) => {
                format!("probe failed (status unchanged): {reason}")
            }
        };
        lines.push(format!("  - {id}: {detail}"));
    }
    if view.healthy == 0 && view.total > 0 {
        lines.push(String::new());
        lines.push(
            "No usable account in the pool. Recovery paths: wait for a muted account to expire, add a new account with `visionary-cli accounts add`, or configure credentials manually."
                .to_string(),
        );
    }
    if let Some(notice) = env_ignored_notice(&config) {
        lines.push(notice);
    }
    for line in &lines {
        println!("{line}");
    }
    Ok(())
}

/// `accounts add`：浏览器登录并登记进池。
///
/// **绕过** `run_login` 的「已配置 token」幂等检查——否则「已有账号时添加第二个号」
/// 永远开不了浏览器。
async fn cmd_accounts_add(args: AccountsJsonArgs) -> Result<()> {
    let config = crate::config::Config::load()?;
    let text = crate::login::run_login_add_account(&config).await?;
    // 登记写回磁盘后重载内存快照，否则 --json 输出的仍是登录前的旧池。
    reload_pool(&config);
    if args.json {
        let view = pool_view(&config);
        println!("{}", serde_json::to_string_pretty(&view)?);
        return Ok(());
    }
    println!("{text}");
    Ok(())
}

/// `accounts remove <id>`：按 id 移除账号（墓碑传播，其它进程不会把它写回）。
fn cmd_accounts_remove(args: AccountsRemoveArgs) -> Result<()> {
    let path = crate::accounts::pool_file()?;
    let pool = crate::accounts::load_pool_from(&path)?;
    let target = pool.find(&args.id).cloned();
    let Some(target) = target else {
        let msg = format!("No such account in the pool: {}", args.id);
        if args.json {
            println!("{}", serde_json::json!({ "error": msg }));
        } else {
            eprintln!("{msg}");
        }
        std::process::exit(1);
    };

    crate::accounts::merge_and_save(&path, |disk| {
        disk.remove(&args.id);
        Ok(())
    })?;

    let config = crate::config::Config::load()?;
    reload_pool(&config);
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "removed": {
                    "id": target.account_id,
                    "label": target.label,
                },
                "pool": pool_view(&config),
            })
        );
        return Ok(());
    }
    println!("Removed account {} ({}).", target.label, target.account_id);
    Ok(())
}

/// `accounts use <id>` / `accounts use --clear`：固定 / 取消固定账号。
fn cmd_accounts_use(args: AccountsUseArgs) -> Result<()> {
    let path = crate::accounts::pool_file()?;
    if args.clear {
        crate::accounts::merge_and_save(&path, |disk| {
            disk.set_pinned(None);
            Ok(())
        })?;
        if args.json {
            let config = crate::config::Config::load()?;
            println!("{}", serde_json::to_string_pretty(&pool_view(&config))?);
            return Ok(());
        }
        println!("Unpinned the account — rotation resumed.");
        return Ok(());
    }

    let Some(id) = args.id.as_deref() else {
        let msg = "`accounts use` needs an account id, or `--clear` to unpin the current account.";
        if args.json {
            println!("{}", serde_json::json!({ "error": msg }));
        } else {
            eprintln!("{msg}");
        }
        std::process::exit(2);
    };

    let pool = crate::accounts::load_pool_from(&path)?;
    if pool.find(id).is_none() {
        let msg = format!("No such account in the pool: {id}");
        if args.json {
            println!("{}", serde_json::json!({ "error": msg }));
        } else {
            eprintln!("{msg}");
        }
        std::process::exit(1);
    }
    crate::accounts::merge_and_save(&path, |disk| {
        disk.set_pinned(Some(id));
        Ok(())
    })?;
    let config = crate::config::Config::load()?;
    reload_pool(&config);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&pool_view(&config))?);
        return Ok(());
    }
    println!("Pinned account {id} — rotation is suspended for it.");
    Ok(())
}

/// `accounts migrate`：把 config.json 里的存量凭据一次性导入账号池。
///
/// 幂等、非破坏、**显式触发**（绝不在启动路径自动跑）。
async fn cmd_accounts_migrate(args: AccountsJsonArgs) -> Result<()> {
    let config = crate::config::Config::load()?;
    let path = crate::accounts::pool_file()?;
    let report = crate::accounts::migrate_from_config(&config, &path).await?;
    reload_pool(&config);
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "imported": report.imported,
                "skipped": report.skipped,
                "reasons": report.reasons,
                "pool": pool_view(&config),
            })
        );
        return Ok(());
    }
    println!("{}", report.summary());
    Ok(())
}
/// 按输出模式输出失败信息并退出非零（design 决策 6）。
///
/// `--json` 模式：stdout 输出原子 `{"error"}`；文本模式：stderr 输出错误文本。
fn fail(mode: OutputMode, msg: &str) -> ! {
    if mode == OutputMode::Json {
        println!("{}", serde_json::json!({ "error": msg }));
    } else {
        eprintln!("{msg}");
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_args_means_help() {
        // arg_required_else_help：无参数时 clap 拒绝解析并输出 help（对应退出码 2）
        let err = Cli::try_parse_from(["visionary-server"])
            .expect_err("no args must be rejected by arg_required_else_help");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        let text = err.to_string();
        assert!(
            text.to_lowercase().contains("usage") || text.contains("mcp-stdio"),
            "help should mention usage/mcp-stdio, got: {text}"
        );
    }

    #[test]
    fn mcp_stdio_subcommand_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "mcp-stdio"]).expect("mcp-stdio parses");
        assert!(matches!(cli.command, Some(Command::McpStdio)));
    }

    #[test]
    fn version_flag_parses() {
        // clap `version` 属性自动提供 -V/--version；--version 短路输出版本号。
        let err = Cli::try_parse_from(["visionary-server", "--version"])
            .expect_err("--version should short-circuit to version output");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        let text = err.to_string();
        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "version output should contain {}, got: {text}",
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn doctor_subcommand_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "doctor"]).expect("doctor parses");
        assert!(matches!(cli.command, Some(Command::Doctor)));
    }

    #[test]
    fn init_positional_parses() {
        let cli =
            Cli::try_parse_from(["visionary-server", "init", "opencode"]).expect("init parses");
        let Some(Command::Init(args)) = cli.command else {
            panic!("expected init subcommand");
        };
        assert_eq!(args.agent.as_deref(), Some("opencode"));
        assert!(!args.dry_run && !args.yes);
    }

    #[test]
    fn init_dsh_positional_parses() {
        let cli =
            Cli::try_parse_from(["visionary-server", "init", "dsh"]).expect("init dsh parses");
        let Some(Command::Init(args)) = cli.command else {
            panic!("expected init subcommand");
        };
        assert_eq!(args.agent.as_deref(), Some("dsh"));
        assert!(!args.dry_run && !args.yes && !args.dsh);
    }

    #[test]
    fn init_flags_parse() {
        let cli = Cli::try_parse_from([
            "visionary-server",
            "init",
            "--opencode",
            "--codex",
            "--yes",
            "--dry-run",
        ])
        .expect("init flags parse");
        let Some(Command::Init(args)) = cli.command else {
            panic!("expected init subcommand");
        };
        assert!(args.opencode && args.codex && args.yes && args.dry_run);
        assert!(!args.claude && !args.cursor && !args.claude_desktop && !args.dsh);
    }

    #[test]
    fn init_dsh_flag_parses() {
        let cli =
            Cli::try_parse_from(["visionary-server", "init", "--dsh"]).expect("init --dsh parses");
        let Some(Command::Init(args)) = cli.command else {
            panic!("expected init subcommand");
        };
        assert!(args.dsh, "--dsh flag should be set");
        assert!(args.agent.is_none());
    }

    #[test]
    fn unknown_subcommand_rejected() {
        assert!(Cli::try_parse_from(["visionary-server", "frobnicate"]).is_err());
    }

    // ---- vision 子命令解析（task 4.1）----

    #[test]
    fn vision_basic_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "vision", "img.png"])
            .expect("vision with image parses");
        let Some(Command::Vision(args)) = cli.command else {
            panic!("expected vision subcommand");
        };
        assert_eq!(args.images, vec!["img.png".to_string()]);
        assert_eq!(args.prompt, "请详细描述这张图片中的内容");
        assert!(
            !args.thinking
                && !args.continue_conversation
                && !args.stream
                && !args.no_stream
                && !args.json
        );
        assert!(args.session_id.is_none());
    }

    #[test]
    fn vision_all_flags_parse() {
        let cli = Cli::try_parse_from([
            "visionary-server",
            "vision",
            "img.png",
            "--prompt",
            "图中有什么？",
            "--thinking",
            "--continue-conversation",
            "--session-id",
            "abc123",
            "--json",
        ])
        .expect("vision all flags parse");
        let Some(Command::Vision(args)) = cli.command else {
            panic!("expected vision subcommand");
        };
        assert_eq!(args.prompt, "图中有什么？");
        assert!(args.thinking && args.continue_conversation && args.json);
        assert_eq!(args.session_id.as_deref(), Some("abc123"));
        assert!(!args.stream && !args.no_stream);
    }

    #[test]
    fn vision_stdin_dash_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "vision", "-"])
            .expect("vision stdin dash parses");
        let Some(Command::Vision(args)) = cli.command else {
            panic!("expected vision subcommand");
        };
        assert_eq!(
            args.images,
            vec!["-".to_string()],
            "`-` means read image from stdin"
        );
    }

    #[test]
    fn vision_multiple_images_parse() {
        let cli = Cli::try_parse_from(["visionary-server", "vision", "a.png", "b.png", "c.png"])
            .expect("vision multiple images parses");
        let Some(Command::Vision(args)) = cli.command else {
            panic!("expected vision subcommand");
        };
        assert_eq!(args.images, vec!["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn vision_missing_image_rejected() {
        assert!(Cli::try_parse_from(["visionary-server", "vision"]).is_err());
    }

    #[test]
    fn vision_stream_no_stream_conflict_rejected() {
        assert!(
            Cli::try_parse_from([
                "visionary-server",
                "vision",
                "x.png",
                "--stream",
                "--no-stream"
            ])
            .is_err(),
            "--stream and --no-stream are mutually exclusive"
        );
    }

    #[test]
    fn vision_json_stream_conflict_rejected() {
        assert!(
            Cli::try_parse_from(["visionary-server", "vision", "x.png", "--json", "--stream"])
                .is_err(),
            "--json and --stream are mutually exclusive"
        );
    }

    #[test]
    fn status_subcommand_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "status"]).expect("status parses");
        let Some(Command::Status(args)) = cli.command else {
            panic!("expected status subcommand");
        };
        assert!(!args.json);
    }

    #[test]
    fn status_json_parses() {
        let cli = Cli::try_parse_from(["visionary-server", "status", "--json"])
            .expect("status --json parses");
        let Some(Command::Status(args)) = cli.command else {
            panic!("expected status subcommand");
        };
        assert!(args.json);
    }

    #[test]
    fn login_logout_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["visionary-server", "login"])
                .unwrap()
                .command,
            Some(Command::Login)
        ));
        assert!(matches!(
            Cli::try_parse_from(["visionary-server", "logout"])
                .unwrap()
                .command,
            Some(Command::Logout)
        ));
    }

    #[test]
    fn skill_subcommand_parses() {
        // `skill install` 可解析；action 省略时默认 install。
        let cli = Cli::try_parse_from(["visionary-server", "skill", "install"])
            .expect("skill install parses");
        let Some(Command::Skill(args)) = cli.command else {
            panic!("expected skill subcommand");
        };
        assert_eq!(args.action.as_deref(), Some("install"));
        let cli = Cli::try_parse_from(["visionary-server", "skill"]).expect("skill parses");
        let Some(Command::Skill(args)) = cli.command else {
            panic!("expected skill subcommand");
        };
        assert!(
            args.action.is_none(),
            "action optional, defaults to install"
        );
    }

    // ---- 输出模式决策纯函数（task 4.2）----

    #[test]
    fn output_mode_tty_defaults_to_stream() {
        assert_eq!(
            resolve_output_mode(true, false, false, false).unwrap(),
            OutputMode::Stream
        );
    }

    #[test]
    fn output_mode_non_tty_defaults_to_atomic_text() {
        assert_eq!(
            resolve_output_mode(false, false, false, false).unwrap(),
            OutputMode::AtomicText
        );
    }

    #[test]
    fn output_mode_stream_flag_overrides_default() {
        // 非 TTY 但 --stream → 强制流式
        assert_eq!(
            resolve_output_mode(false, true, false, false).unwrap(),
            OutputMode::Stream
        );
        // TTY 但 --no-stream → 强制原子文本
        assert_eq!(
            resolve_output_mode(true, false, true, false).unwrap(),
            OutputMode::AtomicText
        );
    }

    #[test]
    fn output_mode_json_is_always_atomic() {
        assert_eq!(
            resolve_output_mode(false, false, false, true).unwrap(),
            OutputMode::Json
        );
        assert_eq!(
            resolve_output_mode(true, false, false, true).unwrap(),
            OutputMode::Json
        );
        assert_eq!(
            resolve_output_mode(true, false, true, true).unwrap(),
            OutputMode::Json,
            "--json + --no-stream 均原子，合法"
        );
    }

    #[test]
    fn output_mode_conflicts_rejected() {
        assert!(resolve_output_mode(true, true, true, false).is_err());
        assert!(resolve_output_mode(true, true, false, true).is_err());
    }

    // ---- modelType / ocr 子命令等价（task 3.3）----

    #[test]
    fn vision_model_type_ocr_parses() {
        let cli = Cli::try_parse_from([
            "visionary-server",
            "vision",
            "img.png",
            "--model-type",
            "ocr",
        ])
        .expect("vision --model-type ocr parses");
        let Some(Command::Vision(args)) = cli.command else {
            panic!("expected vision subcommand");
        };
        assert_eq!(args.model_type, ModelType::Ocr);
        // 与 ocr 子命令共享 handler：等价于 forced=Ocr + 无 --model-type 面
        let ocr = Cli::try_parse_from(["visionary-server", "ocr", "img.png"]).expect("ocr parses");
        let Some(Command::Ocr(ocr_args)) = ocr.command else {
            panic!("expected ocr subcommand");
        };
        let mapped: VisionArgs = ocr_args.into();
        assert_eq!(
            mapped.model_type, args.model_type,
            "must share the ocr pipeline"
        );
        assert_eq!(mapped.images, args.images);
    }

    #[test]
    fn vision_invalid_model_type_rejected() {
        // 非法 modelType 由 clap ValueEnum 拒绝（退出码 2，stderr 提到非法值）
        let err = Cli::try_parse_from([
            "visionary-server",
            "vision",
            "x.png",
            "--model-type",
            "bogus",
        ])
        .expect_err("invalid --model-type must be rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        assert!(err.to_string().contains("bogus"), "unexpected: {err}");
    }

    #[test]
    fn ocr_args_preserve_vision_flag_surface() {
        // ocr 子命令参数面对齐 vision（除 --model-type 外），From<OcrArgs> 完整映射
        let cli = Cli::try_parse_from([
            "visionary-server",
            "ocr",
            "a.png",
            "b.png",
            "--prompt",
            "只提取文字",
            "--thinking",
            "--continue-conversation",
            "--session-id",
            "s1",
            "--json",
        ])
        .expect("ocr all flags parse");
        let Some(Command::Ocr(args)) = cli.command else {
            panic!("expected ocr subcommand");
        };
        let mapped: VisionArgs = args.into();
        assert_eq!(mapped.images, vec!["a.png", "b.png"]);
        assert_eq!(mapped.prompt, "只提取文字");
        assert!(mapped.thinking && mapped.continue_conversation && mapped.json);
        assert_eq!(mapped.session_id.as_deref(), Some("s1"));
        assert_eq!(mapped.model_type, ModelType::Ocr);
    }
}

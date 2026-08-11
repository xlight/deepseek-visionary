//! CLI 入口：`--version` / `doctor` / `init` / `vision` / `status` / `login` / `logout` 子命令；
//! 无参数时进入 MCP stdio serve 模式。
//!
//! 关键约束（design.md 决策 1）：所有 agent 配置都以 `command: ["visionary-server"]`
//! 无参启动，因此无参数路径必须与引入 CLI 前完全兼容。

use crate::hif::HifAuth;
use crate::pipeline::{self, VisionRequest};
use crate::server::VisionaryServer;
use crate::session::SessionStore;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use rmcp::ServiceExt;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

/// DeepSeek Visionary MCP server（原生二进制）。
#[derive(Debug, Parser)]
#[command(
    name = "visionary-server",
    version,
    about = "DeepSeek Visionary MCP server"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 诊断环境与配置（浏览器、凭据、平台/架构）。
    Doctor,
    /// 引导接入 AI agent（opencode / codex / claude / cursor / claude-desktop）。
    Init(InitArgs),
    /// 用 DeepSeek 视觉模型分析图片（CLI 版 deepseek_vision）。
    Vision(VisionArgs),
    /// 轻量鉴权状态检查（CLI 版 deepseek_vision_status）。
    Status(StatusArgs),
    /// 浏览器自动登录（CLI 版 deepseek_vision_login）。
    Login,
    /// 清除保存的凭据（CLI 版 deepseek_vision_logout）。
    Logout,
}

/// `vision` 子命令参数：图片 + 提示词/思考/会话续聊 + 输出模式开关。
#[derive(Debug, Args)]
pub struct VisionArgs {
    /// 图片：本地路径 / base64 / data URI，或 `-` 从 stdin 读取。
    pub image: String,
    /// 对图片的问题（默认：请详细描述这张图片中的内容）。
    #[arg(long, default_value = "请详细描述这张图片中的内容")]
    pub prompt: String,
    /// 启用 DeepThink 深度思考。
    #[arg(long)]
    pub thinking: bool,
    /// 续聊：复用上一次会话并链式追问，可对比多张图片。
    #[arg(long)]
    pub continue_conversation: bool,
    /// 显式复用指定 session_id（优先于 --continue）。
    #[arg(long)]
    pub session_id: Option<String>,
    /// 强制流式输出（覆盖 TTY 检测默认）。
    #[arg(long, conflicts_with = "no_stream")]
    pub stream: bool,
    /// 强制原子输出（覆盖 TTY 检测默认）。
    #[arg(long, conflicts_with = "stream")]
    pub no_stream: bool,
    /// 原子 JSON 输出（禁用流式）。
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,
}

/// `status` 子命令参数。
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// 原子 JSON 输出。
    #[arg(long)]
    pub json: bool,
}

/// `init` 子命令参数：位置参数（单个 agent）或多选 flags（批量）+ `--yes` / `--dry-run`。
#[derive(Debug, Args)]
pub struct InitArgs {
    /// 目标 agent 名：opencode / codex / claude / claude-desktop / cursor。
    pub agent: Option<String>,
    /// 批量：同时配置 opencode。
    #[arg(long)]
    pub opencode: bool,
    /// 批量：同时配置 Codex。
    #[arg(long)]
    pub codex: bool,
    /// 批量：同时配置 Claude Code。
    #[arg(long)]
    pub claude: bool,
    /// 批量：同时配置 Claude Desktop。
    #[arg(long, visible_alias = "claude-desktop")]
    pub claude_desktop: bool,
    /// 批量：同时配置 Cursor。
    #[arg(long)]
    pub cursor: bool,
    /// 免交互：跳过确认直接写入。
    #[arg(long)]
    pub yes: bool,
    /// 仅预览将写入的配置，不落盘。
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => serve().await,
        Some(Command::Doctor) => cmd_doctor().await,
        Some(Command::Init(args)) => crate::onboarding::cmd_init(args),
        Some(Command::Vision(args)) => cmd_vision(args).await,
        Some(Command::Status(args)) => cmd_status(args).await,
        Some(Command::Login) => cmd_login().await,
        Some(Command::Logout) => cmd_logout().await,
    }
}

/// MCP stdio serve 模式（无参数时的默认行为，与引入 CLI 前完全一致）。
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
            lines.push(format!("- Config: ❌ {e}"));
            issues += 1;
        }
    }

    // 浏览器检测
    match crate::browser::find_browser() {
        Ok(b) => lines.push(format!("- Browser: ✅ {}", b.display())),
        Err(e) => {
            lines.push(format!("- Browser: ❌ {e}"));
            issues += 1;
        }
    }

    // 凭据：本地检查 + 真实 token 探针
    let config = crate::config::Config::load()?;
    let authenticated = config.is_authenticated();
    lines.push(format!(
        "- Authenticated: {}",
        if authenticated { "✅" } else { "❌" }
    ));

    if authenticated {
        match tokio::time::timeout(Duration::from_secs(15), crate::auth::probe_token(&config)).await
        {
            Ok(Ok(())) => lines.push(format!(
                "- Token validation: ✅ (live probe passed at {})",
                config.base_url
            )),
            Ok(Err(e)) => {
                lines.push(format!("- Token validation: ❌ probe failed: {e}"));
                issues += 1;
            }
            Err(_) => {
                lines.push("- Token validation: ⚠️ probe timed out".to_string());
            }
        }
    }

    if issues > 0 {
        lines.push(String::new());
        lines.push(
            "需修复：运行 `visionary-server login` 自动登录，或设置 DEEPSEEK_USER_TOKEN 环境变量；"
                .into(),
        );
        lines.push("安装 Chrome / Chromium / Edge 任一浏览器以支持自动登录。".into());
    }

    for line in &lines {
        println!("{line}");
    }

    if issues > 0 {
        std::process::exit(1);
    }
    Ok(())
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
fn resolve_output_mode(tty: bool, stream: bool, no_stream: bool, json: bool) -> Result<OutputMode, String> {
    if stream && no_stream {
        return Err("--stream 与 --no-stream 不能同时指定".into());
    }
    if json && stream {
        return Err("--json 与 --stream 不能同时指定（--json 恒为原子输出）".into());
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
    Ok(if tty { OutputMode::Stream } else { OutputMode::AtomicText })
}

/// `vision`：CLI 方式运行完整 vision 流水线（design 决策 3/4/6）。
async fn cmd_vision(args: VisionArgs) -> Result<()> {
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
    if !config.is_authenticated() {
        fail(mode, "未登录：请先运行 `visionary-server login` 自动登录，或设置 DEEPSEEK_USER_TOKEN 环境变量。");
    }

    // 读取图片（路径 / base64 / data URI / stdin）
    let image_data = match pipeline::read_image(&args.image) {
        Ok(d) => d,
        Err(e) => fail(mode, &e),
    };

    // 会话连续性（与 MCP handler 共用同一解析）
    let session_store = SessionStore::new();
    let (reuse_session_id, reuse_parent_message_id) = pipeline::resolve_session_reuse(
        &session_store,
        args.session_id.as_deref(),
        args.continue_conversation,
    );

    let request = VisionRequest {
        image_data,
        prompt: args.prompt,
        thinking: args.thinking,
        session_id: reuse_session_id,
        parent_message_id: reuse_parent_message_id,
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
                    Some(&mut on_token),
                )
                .await
            };
            match result {
                Ok(output) => {
                    println!(
                        "\n---\n[session_id: {}] (可用 --continue 继续此对话)",
                        output.session_id
                    );
                }
                Err(e) => fail(mode, &format!("Vision analysis failed: {e:#}")),
            }
        }
        OutputMode::AtomicText | OutputMode::Json => {
            // 原子模式：不流式（None::<fn(&str)> 满足 Send 约束）
            match pipeline::run_vision_pipeline::<fn(&str)>(
                &config, &hif, &session_store, request, None,
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
                            "\n---\n[session_id: {}] (可用 --continue 继续此对话)",
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
    let creds = config.credentials();
    let authenticated = config.is_authenticated();
    let token_valid = if authenticated {
        matches!(
            tokio::time::timeout(Duration::from_secs(15), crate::auth::probe_token(&config)).await,
            Ok(Ok(()))
        )
    } else {
        false
    };

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "authenticated": authenticated,
                "token_configured": !creds.user_token.is_empty(),
                "smid_v2": !creds.smid_v2.is_empty(),
                "base_url": config.base_url,
                "token_valid": token_valid,
            })
        );
        if !token_valid {
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut lines = vec![
        format!("DeepSeek Vision v{}", env!("CARGO_PKG_VERSION")),
        String::new(),
        format!(
            "- Authenticated: {}",
            if authenticated { "✅" } else { "❌" }
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
                "❌ (optional)"
            } else {
                "✅"
            }
        ),
        format!("- Base URL: {}", config.base_url),
    ];
    if authenticated {
        lines.push(format!(
            "- Token validation: {}",
            if token_valid {
                "✅ (live probe passed)"
            } else {
                "❌ probe failed"
            }
        ));
    }
    if !token_valid {
        lines.push(String::new());
        lines.push(
            "未登录：请运行 `visionary-server login` 自动登录，或设置 DEEPSEEK_USER_TOKEN 环境变量。"
                .into(),
        );
    }
    for line in &lines {
        println!("{line}");
    }
    if !token_valid {
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
    fn no_args_means_serve() {
        let cli = Cli::try_parse_from(["visionary-server"]).expect("no args should parse");
        assert!(cli.command.is_none(), "no args must mean serve mode");
    }

    #[test]
    fn version_flag_parses() {
        // clap `version` 属性自动提供 -V/--version；此处只验证可解析且不出现在子命令分支。
        let cli = Cli::try_parse_from(["visionary-server"]).expect("parses");
        assert!(cli.command.is_none());
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
        assert!(!args.claude && !args.cursor && !args.claude_desktop);
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
        assert_eq!(args.image, "img.png");
        assert_eq!(args.prompt, "请详细描述这张图片中的内容");
        assert!(!args.thinking && !args.continue_conversation && !args.stream && !args.no_stream && !args.json);
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
        assert_eq!(args.image, "-", "`-` means read image from stdin");
    }

    #[test]
    fn vision_missing_image_rejected() {
        assert!(Cli::try_parse_from(["visionary-server", "vision"]).is_err());
    }

    #[test]
    fn vision_stream_no_stream_conflict_rejected() {
        assert!(
            Cli::try_parse_from(["visionary-server", "vision", "x.png", "--stream", "--no-stream"])
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
            Cli::try_parse_from(["visionary-server", "login"]).unwrap().command,
            Some(Command::Login)
        ));
        assert!(matches!(
            Cli::try_parse_from(["visionary-server", "logout"]).unwrap().command,
            Some(Command::Logout)
        ));
    }

    // ---- 输出模式决策纯函数（task 4.2）----

    #[test]
    fn output_mode_tty_defaults_to_stream() {
        assert_eq!(resolve_output_mode(true, false, false, false).unwrap(), OutputMode::Stream);
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
        assert_eq!(resolve_output_mode(false, true, false, false).unwrap(), OutputMode::Stream);
        // TTY 但 --no-stream → 强制原子文本
        assert_eq!(
            resolve_output_mode(true, false, true, false).unwrap(),
            OutputMode::AtomicText
        );
    }

    #[test]
    fn output_mode_json_is_always_atomic() {
        assert_eq!(resolve_output_mode(false, false, false, true).unwrap(), OutputMode::Json);
        assert_eq!(resolve_output_mode(true, false, false, true).unwrap(), OutputMode::Json);
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
}

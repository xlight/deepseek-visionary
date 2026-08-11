//! CLI 入口：`--version` / `mcp-stdio` / `doctor` / `init` / `vision` / `status` /
//! `login` / `logout` / `skill` 子命令；无参数时输出 help 并退出（clap
//! `arg_required_else_help`，退出码 2）。
//!
//! 关键约束（design.md 决策 1）：MCP stdio 模式必须显式 `mcp-stdio` 子命令启动；
//! 所有 agent 配置都以 `command: ["visionary-server", "mcp-stdio"]` 启动。

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

/// DeepSeek Visionary MCP server (native binary).
#[derive(Debug, Parser)]
#[command(
    name = "visionary-server",
    version,
    about = "DeepSeek Visionary MCP server",
    arg_required_else_help = true,
    after_help = "MCP stdio mode: run `visionary-server mcp-stdio` to start the MCP server.\n\
                  Existing agent configs start the server without args; re-run `visionary-server init` to migrate them."
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
    /// Bootstrap AI agent integration (opencode / codex / claude / cursor / claude-desktop).
    Init(InitArgs),
    /// Analyze an image with DeepSeek's vision model (CLI counterpart of deepseek_vision).
    Vision(VisionArgs),
    /// Lightweight auth status check (CLI counterpart of deepseek_vision_status).
    Status(StatusArgs),
    /// Browser auto-login (CLI counterpart of deepseek_vision_login).
    Login,
    /// Clear saved credentials (CLI counterpart of deepseek_vision_logout).
    Logout,
    /// Install the agent-calling-contract skill (embedded in the binary).
    Skill(SkillArgs),
}

/// `vision` 子命令参数：图片 + 提示词/思考/会话续聊 + 输出模式开关。
#[derive(Debug, Args)]
pub struct VisionArgs {
    /// Image: local path / base64 / data URI, or `-` to read from stdin.
    pub image: String,
    /// Question about the image (default: detailed description in Chinese).
    #[arg(long, default_value = "请详细描述这张图片中的内容")]
    pub prompt: String,
    /// Enable DeepThink deep reasoning.
    #[arg(long)]
    pub thinking: bool,
    /// Continue the session for multi-image comparison and follow-up questions.
    #[arg(long)]
    pub continue_conversation: bool,
    /// Explicitly reuse a session_id (takes precedence over --continue).
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

/// `status` 子命令参数。
#[derive(Debug, Args)]
pub struct StatusArgs {
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
    /// Target agent name: opencode / codex / claude / claude-desktop / cursor.
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
        Some(Command::Vision(args)) => cmd_vision(args).await,
        Some(Command::Status(args)) => cmd_status(args).await,
        Some(Command::Login) => cmd_login().await,
        Some(Command::Logout) => cmd_logout().await,
        Some(Command::Skill(args)) => cmd_skill(args),
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

    // 凭据：本地检查 + 真实 token 探针
    let config = crate::config::Config::load()?;
    let authenticated = config.is_authenticated();
    lines.push(format!(
        "- Authenticated: {}",
        if authenticated { "[OK]" } else { "[FAIL]" }
    ));

    if authenticated {
        match tokio::time::timeout(Duration::from_secs(15), crate::auth::probe_token(&config)).await
        {
            Ok(Ok(())) => lines.push(format!(
                "- Token validation: [OK] (live probe passed at {})",
                config.base_url
            )),
            Ok(Err(e)) => {
                lines.push(format!("- Token validation: [FAIL] probe failed: {e}"));
                issues += 1;
            }
            Err(_) => {
                lines.push("- Token validation: [WARN] probe timed out".to_string());
            }
        }
    }

    if issues > 0 {
        lines.push(String::new());
        lines.push(
            "To fix: run `visionary-server login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable."
                .into(),
        );
        lines
            .push("Install Chrome / Chromium / Edge for auto-login support.".into());
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
        fail(
            mode,
            "Not logged in: run `visionary-server login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable.",
        );
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
                        "\n---\n[session_id: {}] (use --continue to keep chatting)",
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
                            "\n---\n[session_id: {}] (use --continue to keep chatting)",
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
    if !token_valid {
        lines.push(String::new());
        lines.push(
            "Not logged in: run `visionary-server login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable."
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

/// 内嵌的 agent 调用契约 SKILL.md（随二进制分发，版本必然匹配）。
const EMBEDDED_SKILL: &str = include_str!("../../../skills/visionary-cli/SKILL.md");

/// `skill install`：把内嵌 SKILL.md 写入 `~/.agents/skills/visionary-cli/SKILL.md`。
///
/// design 决策 7：通过安装脚本（cargo-dist / brew / npm）装二进制的用户本地没有仓库，
/// 无法 `cp -r skills/...`；内嵌保证安装二进制即具备 skill，已存在时覆盖并提示。
fn cmd_skill(args: SkillArgs) -> Result<()> {
    match args.action.as_deref() {
        Some("install") | None => {}
        Some(other) => {
            eprintln!("Unknown skill action: {other} (only `install` is supported)");
            std::process::exit(1);
        }
    }

    // ~/.agents/skills/visionary-cli/（镜像数据目录逻辑，但固定到 ~/.agents）
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = home.join(".agents").join("skills").join("visionary-cli");
    std::fs::create_dir_all(&dir).context("create skill directory")?;
    let path = dir.join("SKILL.md");
    let existed = path.exists();
    std::fs::write(&path, EMBEDDED_SKILL).context("write SKILL.md")?;
    if existed {
        println!("Skill updated: {}", path.display());
    } else {
        println!("Skill installed: {}", path.display());
    }
    println!(
        "Tip: if your agent's default skills dir is not ~/.agents/skills (e.g. Claude Code uses ~/.claude/skills),\n\
         move the visionary-cli directory to that agent's default dir, e.g.:\n\
         mv {} ~/.claude/skills/",
        dir.display()
    );
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
        let cli = Cli::try_parse_from(["visionary-server", "mcp-stdio"])
            .expect("mcp-stdio parses");
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
        assert!(args.action.is_none(), "action optional, defaults to install");
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

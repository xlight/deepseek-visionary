//! CLI 入口：`--version` / `doctor` / `init` 子命令；无参数时进入 MCP stdio serve 模式。
//!
//! 关键约束（design.md 决策 1）：所有 agent 配置都以 `command: ["visionary-server"]`
//! 无参启动，因此无参数路径必须与引入 CLI 前完全兼容。

use crate::server::VisionaryServer;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use rmcp::ServiceExt;
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
            "需修复：运行 `deepseek_vision_login` 自动登录，或设置 DEEPSEEK_USER_TOKEN 环境变量；"
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
}

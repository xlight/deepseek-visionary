//! visionary-server — DeepSeek Visionary MCP server（原生二进制）。
//!
//! 职责（对照 Python 版 `deepseek_vision_mcp` 各模块）：
//! - config / auth / pow / upload / hif / completion / session / pipeline：核心 vision 流水线
//! - MCP stdio 服务：`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`
//! - CDP 浏览器自动登录
//! - CLI 子命令：`--version` / `doctor` / `init`（无参数时进入 MCP serve 模式）

mod auth;
mod browser;
mod cli;
mod client;
mod completion;
mod config;
mod fork;
mod hif;
mod login;
mod onboarding;
mod pipeline;
mod pow;
mod server;
mod session;
mod upload;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 日志必须走 stderr——stdout 是 MCP stdio 协议通道，不能被污染。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    cli::run().await
}

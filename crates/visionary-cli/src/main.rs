//! visionary-cli — DeepSeek Visionary CLI（原生二进制）。
//!
//! 职责（对照 Python 版 `deepseek_vision_mcp` 各模块）：
//! - config / auth / pow / upload / hif / completion / session / pipeline：核心 vision 流水线
//! - MCP stdio 服务：`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`
//! - CDP 浏览器自动登录
//! - CLI 子命令：`--version` / `doctor` / `init`（无参数时进入 MCP serve 模式）

mod account_health;
mod accounts;
mod auth;
mod browser;
mod cli;
mod client;
mod completion;
mod config;
mod hif;
mod integrity;
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
    // 默认过滤 chromiumoxide 的 CDP 解析告警（新版 Chrome 会发送 chromiumoxide 0.9
    // 尚未识别的消息，逐条刷屏但无害）；用户显式设置 RUST_LOG 时按用户配置走。
    let filter = match tracing_subscriber::EnvFilter::try_from_default_env() {
        Ok(f) => f,
        Err(_) => tracing_subscriber::EnvFilter::new("info,chromiumoxide::handler=error"),
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    cli::run().await
}

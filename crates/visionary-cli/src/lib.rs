//! visionary-cli — DeepSeek Visionary CLI 的实现库。
//!
//! 二进制入口有两个：`src/bin/visionary-cli.rs`（主名）与
//! `src/bin/visionary-server.rs`（旧名别名，双名过渡期保留）。两者都只调用
//! [`run`]，因此共享同一份实现与同一个 MCP 服务逻辑。
//!
//! 职责（对照 Python 版 `deepseek_vision_mcp` 各模块）：
//! - config / auth / pow / upload / hif / completion / session / pipeline：核心 vision 流水线
//! - MCP stdio 服务：`deepseek_vision` / `deepseek_vision_status` / `deepseek_vision_login` / `deepseek_vision_logout`
//! - CDP 浏览器自动登录
//! - CLI 子命令：`--version` / `doctor` / `init`（无参数时进入 MCP serve 模式）

pub mod account_health;
pub mod accounts;
pub mod auth;
pub mod browser;
pub mod cli;
pub mod client;
pub mod completion;
pub mod config;
pub mod hif;
pub mod integrity;
pub mod login;
pub mod onboarding;
pub mod pipeline;
pub mod pow;
pub mod server;
pub mod session;
pub mod upload;

use anyhow::Result;

/// 初始化日志并进入 CLI 主流程。
///
/// 日志必须走 stderr——stdout 是 MCP stdio 协议通道，不能被污染。
/// 默认过滤 chromiumoxide 的 CDP 解析告警（新版 Chrome 会发送 chromiumoxide 0.9
/// 尚未识别的消息，逐条刷屏但无害）；用户显式设置 RUST_LOG 时按用户配置走。
pub async fn run() -> Result<()> {
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

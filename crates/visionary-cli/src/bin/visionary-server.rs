//! 旧名二进制入口：`visionary-server`（双名过渡期兼容用）。
//!
//! 与 `visionary-cli` 完全相同——同一份库、同一 `run`。保留此入口是为了让
//! v0.8.0 的既有安装、文档与 agent 配置（`command: ["visionary-server", "mcp-stdio"]`）
//! 继续可用；新配置应使用 `visionary-cli`。

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(visionary_cli::run())
}

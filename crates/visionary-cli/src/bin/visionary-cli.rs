//! 主二进制入口：`visionary-cli`。
//!
//! 全部实现位于本 crate 的库（`lib.rs`）；旧名 `visionary-server` 是另一个
//! 指向同一 `run` 的薄入口，用于双名过渡期兼容 v0.8.0 的既有配置。

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(visionary_cli::run())
}

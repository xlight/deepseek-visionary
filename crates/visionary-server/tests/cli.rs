//! 二进制级回归测试：--version / 未知子命令 / 无参数 serve 握手。
//!
//! 关键约束（design.md 决策 1）：无参数启动必须进入 MCP stdio serve 模式，
//! 与引入 CLI 前完全兼容——这里用真实 MCP initialize 握手验证。

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_visionary-server")
}

#[test]
fn version_flag_prints_version() {
    let out = Command::new(bin()).arg("--version").output().expect("run");
    assert!(out.status.success(), "--version should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("visionary-server"),
        "stdout should contain binary name, got: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "stdout should contain version {}, got: {stdout}",
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn unknown_subcommand_fails_without_serving() {
    // 未知子命令必须立即失败（不进入 serve 阻塞），stderr 有用法提示，退出非零。
    let out = Command::new(bin()).arg("frobnicate").output().expect("run");
    assert!(
        !out.status.success(),
        "unknown subcommand should exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("error") || stderr.contains("usage"),
        "stderr should contain usage/error, got: {stderr}"
    );
}

#[test]
fn no_args_starts_mcp_serve_and_handshakes() {
    // 无参数 = MCP stdio serve。写一个最小 initialize 请求，读回响应，
    // 确认服务正常启动且不因 CLI 引入而破坏协议。
    let mut child = Command::new(bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"cli-test","version":"0.0.1"}}}"#;
    stdin
        .write_all(format!("{init}\n").as_bytes())
        .expect("write initialize");
    stdin.flush().expect("flush");

    let mut resp = String::new();
    stdout
        .read_line(&mut resp)
        .expect("read initialize response");
    assert!(
        resp.contains("\"jsonrpc\"") && resp.contains("\"result\""),
        "expected MCP initialize response, got: {resp}"
    );

    // 关掉 stdin，服务应退出
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn doctor_subcommand_exits_cleanly() {
    // doctor 不应进入 serve 阻塞：必须在有限时间内退出（成功或非零都算正常退出）。
    let out = Command::new(bin()).arg("doctor").output().expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DeepSeek Visionary"),
        "doctor should print header, got: {stdout}"
    );
    assert!(
        stdout.contains("Browser") || stdout.contains("Config"),
        "doctor should print diagnostics, got: {stdout}"
    );
}

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

/// 构造一个隔离 HOME 的 Command（不触碰真实 ~/.deepseek-visionary 配置）。
///
/// 用进程 id 保证并行测试间目录唯一。
fn isolated_cmd() -> Command {
    let home = std::env::temp_dir().join(format!(
        "visionary-cli-test-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("t")
    ));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::create_dir_all(&home);
    let mut cmd = Command::new(bin());
    cmd.env("HOME", &home);
    cmd
}

#[test]
fn status_json_unauthenticated_exits_nonzero_with_shape() {
    // 隔离 HOME（无凭据）：status --json 输出完整状态 JSON（token_valid=false），退出非零。
    let out = isolated_cmd().args(["status", "--json"]).output().expect("run");
    assert!(
        !out.status.success(),
        "unauthenticated status --json should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("status --json should be valid JSON");
    assert_eq!(v["authenticated"], false);
    assert_eq!(v["token_configured"], false);
    assert_eq!(v["token_valid"], false);
    assert!(v["base_url"].is_string(), "base_url should be present");
}

#[test]
fn status_text_unauthenticated_exits_nonzero() {
    // 隔离 HOME（无凭据）：status 文本模式输出未认证提示，退出非零。
    let out = isolated_cmd().arg("status").output().expect("run");
    assert!(
        !out.status.success(),
        "unauthenticated status should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Authenticated"),
        "status text should report auth state, got: {stdout}"
    );
}

#[test]
fn logout_unauthenticated_succeeds() {
    // 隔离 HOME（无凭据）：logout 清除空凭据，输出结果，退出 0。
    let out = isolated_cmd().arg("logout").output().expect("run");
    assert!(out.status.success(), "logout should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("credentials cleared"),
        "logout should print clear confirmation, got: {stdout}"
    );
}

#[test]
fn vision_unauthenticated_text_mode_exits_nonzero() {
    // 隔离 HOME（无凭据）+ 显式 --no-stream（不依赖 CI 的 TTY 探测）：
    // vision 未登录 → stderr 登录指引，退出非零，不进入 serve。
    let out = isolated_cmd()
        .args(["vision", "x.png", "--no-stream"])
        .output()
        .expect("run");
    assert!(
        !out.status.success(),
        "unauthenticated vision should exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("visionary-server login"),
        "stderr should guide login, got: {stderr}"
    );
}

#[test]
fn vision_unauthenticated_json_mode_exits_nonzero_with_error() {
    // 隔离 HOME（无凭据）：vision --json 未登录 → stdout 原子 {"error"}，退出非零。
    let out = isolated_cmd()
        .args(["vision", "x.png", "--json"])
        .output()
        .expect("run");
    assert!(
        !out.status.success(),
        "unauthenticated vision --json should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("vision --json error should be valid JSON");
    assert!(
        v["error"].is_string(),
        "vision --json error should have error field, got: {stdout}"
    );
}

#[test]
fn skill_install_writes_embedded_skill() {
    // 隔离 HOME：skill install 写入 ~/.agents/skills/visionary-cli/SKILL.md，退出 0。
    let out = isolated_cmd().args(["skill", "install"]).output().expect("run");
    assert!(out.status.success(), "skill install should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("SKILL.md"), "should print path, got: {stdout}");

    // 验证文件已写入且内容非空（以 --- 开头的 YAML frontmatter 开头）
    let home = std::env::temp_dir().join(format!(
        "visionary-cli-test-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("t")
    ));
    let path = home.join(".agents/skills/visionary-cli/SKILL.md");
    let content = std::fs::read_to_string(&path).expect("SKILL.md should exist");
    assert!(
        content.starts_with("---\nname: visionary-cli"),
        "SKILL.md should start with frontmatter, got: {}",
        &content[..content.len().min(60)]
    );
}

#[test]
fn skill_install_overwrites_existing() {
    // 隔离 HOME：重复 skill install 覆盖既有文件，仍退出 0 且内容一致。
    let mut cmd = isolated_cmd();
    cmd.args(["skill", "install"]);
    let first = cmd.output().expect("first run");
    assert!(first.status.success());
    let second = cmd.output().expect("second run");
    assert!(second.status.success(), "re-run should exit 0");
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("Skill updated"),
        "re-run should print updated hint, got: {stdout}"
    );
}

#[test]
fn skill_unknown_action_fails() {
    // 隔离 HOME：未知 skill 操作报错退出非零。
    let out = isolated_cmd().args(["skill", "frobnicate"]).output().expect("run");
    assert!(
        !out.status.success(),
        "unknown skill action should exit non-zero"
    );
}

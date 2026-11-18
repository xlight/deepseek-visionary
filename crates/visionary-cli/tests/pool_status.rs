//! 账号池模式的二进制级回归测试（池概况 / 退出码 / 环境变量忽略）。
//!
//! 关键约束（任务 1.4d / 1.7）：
//! - 池模式下「未登录」判据以「池中有无可用账号」为准——兼容配置为空或探针失败
//!   但池里有可用账号时，`status` SHALL NOT 退出非零；
//! - 池存在时 `DEEPSEEK_USER_TOKEN` 等环境变量不参与选号，`status` SHALL 明确提示已忽略。
//!
//! 用隔离的 HOME / USERPROFILE，避免触碰真实 `~/.deepseek-visionary`。

use std::io::{Read, Write};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_visionary-cli")
}

/// 隔离用户目录的 Command（Windows 也要隔离：USERPROFILE 才是 home_dir 的来源）。
fn isolated_cmd(home: &std::path::Path) -> Command {
    // 注意：这里**不能**删除 home（测试会先往里写账号池文件）；
    // 清理由调用方在写文件之前完成。
    std::fs::create_dir_all(home).expect("create isolated home");
    let mut cmd = Command::new(bin());
    cmd.env("HOME", home).env("USERPROFILE", home);
    cmd
}

/// 写入账号池文件。
fn write_pool(home: &std::path::Path, accounts: &[&str]) {
    let _ = std::fs::remove_dir_all(home);
    let dir = home.join(".deepseek-visionary");
    std::fs::create_dir_all(&dir).expect("create data dir");
    let body = format!(
        "{{\"accounts\":[{}],\"cursor\":0,\"strategy\":\"rotate\",\"pinned\":null,\"removed\":[]}}",
        accounts.join(",")
    );
    std::fs::write(dir.join("accounts.json"), body).expect("write pool");
}

/// 启动一个最小 mock HTTP 服务：任何 GET 都返回可解析的探针响应。
fn spawn_mock() -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            let _ = tx.send(line);
            let body = r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"acc-1","email":"a@b.c","chat":{"is_muted":0,"mute_until":0,"msg":null}}}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}"), rx)
}

#[test]
fn status_in_pool_mode_reports_pool_and_ignores_env_token() {
    let home = std::env::temp_dir().join(format!("visionary-pool-status-{}", std::process::id()));
    let (base_url, _rx) = spawn_mock();
    write_pool(
        &home,
        &[
            r#"{"id":"acc-1","label":"a@b.c","user_token":"tok-1","smid_v2":"","cf_clearance":"","status":"healthy","mute_until":null,"last_used":null}"#,
            r#"{"id":"acc-2","label":"x@y.z","user_token":"tok-2","smid_v2":"","cf_clearance":"","status":"healthy","mute_until":null,"last_used":null}"#,
        ],
    );

    let out = isolated_cmd(&home)
        .arg("status")
        .arg("--json")
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("DEEPSEEK_USER_TOKEN", "env-token-should-be-ignored")
        .output()
        .expect("run status");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    // 账号池概况是**新增子对象** `pool`（既有顶层字段保持不变）。
    assert_eq!(
        v["pool"]["enabled"],
        serde_json::Value::Bool(true),
        "stdout: {stdout}"
    );
    assert_eq!(v["pool"]["total"], serde_json::json!(2), "stdout: {stdout}");
    assert_eq!(
        v["pool"]["healthy"],
        serde_json::json!(2),
        "stdout: {stdout}"
    );
    assert_eq!(v["pool"]["muted"], serde_json::json!(0), "stdout: {stdout}");
    assert_eq!(
        v["pool"]["expired"],
        serde_json::json!(0),
        "stdout: {stdout}"
    );
    assert_eq!(
        v["pool"]["env_ignored"],
        serde_json::Value::Bool(true),
        "env token should be reported as ignored: {stdout}"
    );
    assert!(
        out.status.success(),
        "pool with usable accounts must exit 0: {} / {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn status_in_pool_mode_exits_zero_without_compat_config() {
    // 兼容配置为空（没有 config.json）但池里有可用账号 → 不得报「未登录」、不得退出非零。
    let home = std::env::temp_dir().join(format!("visionary-pool-nocompat-{}", std::process::id()));
    let (base_url, _rx) = spawn_mock();
    write_pool(
        &home,
        &[
            r#"{"id":"acc-1","label":"a@b.c","user_token":"tok-1","smid_v2":"","cf_clearance":"","status":"healthy","mute_until":null,"last_used":null}"#,
        ],
    );

    let out = isolated_cmd(&home)
        .arg("status")
        .arg("--json")
        .env("DEEPSEEK_BASE_URL", &base_url)
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "pool with usable accounts must exit 0 even without config.json: {} / {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(
        v["pool"]["enabled"],
        serde_json::Value::Bool(true),
        "stdout: {stdout}"
    );
    assert_eq!(
        v["pool"]["healthy"],
        serde_json::json!(1),
        "stdout: {stdout}"
    );
}

#[test]
fn status_without_pool_keeps_single_account_semantics() {
    // 没有 accounts.json → 单账号模式：pool_mode=false，token 未配置时退出非零。
    let home = std::env::temp_dir().join(format!("visionary-single-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create isolated home");
    let (base_url, _rx) = spawn_mock();
    let out = isolated_cmd(&home)
        .arg("status")
        .arg("--json")
        .env("DEEPSEEK_BASE_URL", &base_url)
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json output");
    assert_eq!(
        v["pool"]["enabled"],
        serde_json::Value::Bool(false),
        "stdout: {stdout}"
    );
    assert!(
        !out.status.success(),
        "single-account mode without token must exit non-zero: {stdout}"
    );
}

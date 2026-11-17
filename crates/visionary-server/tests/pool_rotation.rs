//! 账号池轮询的二进制级集成测试（假 HTTP 端点驱动）。
//!
//! 覆盖任务 10.3 / 10.4：
//! - **轮换**：连续请求使用不同账号（Authorization 头不同）；
//! - **跳过静音**：池中已标记 muted 的账号不会被选中；
//! - **自动切换重试**：账号级失败（biz_code=14 上传风控）后自动换下一个账号，
//!   且把失败账号标记为不可用并落盘；
//! - **全不可用**：池中没有可用账号时明确报错。
//!
//! 假服务端只实现流水线在「上传」之前/之时用到的两个端点：
//! `POST /api/v0/chat/create_pow_challenge`（合成 challenge）与
//! `POST /api/v0/file/upload_file`（按 Authorization 决定是否风控）。
//! 上传失败即终止流水线，因此无需模拟会话 / HIF / completion。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_visionary-server")
}

/// 隔离用户目录：放在工作区 target 下（保证子进程可写）。
fn isolated_home(tag: &str) -> PathBuf {
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(".pool-rotation-test")
        .join(tag);
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create isolated home");
    home
}

/// 写账号池文件（accounts 为预渲染的 JSON 片段）。
fn write_pool(home: &Path, accounts: &[&str]) {
    let dir = home.join(".deepseek-visionary");
    std::fs::create_dir_all(&dir).expect("create data dir");
    let body = format!(
        "{{\"accounts\":[{}],\"cursor\":0,\"strategy\":\"rotate\",\"pinned\":null,\"removed\":[]}}",
        accounts.join(",")
    );
    std::fs::write(dir.join("accounts.json"), body).expect("write pool");
}

fn account_json(id: &str, token: &str, status: &str, mute_until: Option<i64>) -> String {
    let mute = match mute_until {
        Some(v) => v.to_string(),
        None => "null".to_string(),
    };
    format!(
        "{{\"id\":\"{id}\",\"label\":\"{id}@example.com\",\"user_token\":\"{token}\",\"smid_v2\":\"\",\"cf_clearance\":\"\",\"status\":\"{status}\",\"mute_until\":{mute},\"last_used\":null}}"
    )
}

/// 假服务端：返回 (base_url, 请求记录接收端)。
/// 记录格式：`"<METHOD> <PATH>\t<Authorization>"`。
fn spawn_mock(muted_tokens: &[&str]) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    let (tx, rx) = mpsc::channel();
    let muted: Vec<String> = muted_tokens.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf: Vec<u8> = Vec::new();
            let mut tmp = [0u8; 4096];
            let mut head_end: Option<usize> = None;
            let mut content_length = 0usize;
            while let Ok(n) = stream.read(&mut tmp) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if head_end.is_none() {
                    if let Some(pos) = find(&buf, b"\r\n\r\n") {
                        head_end = Some(pos + 4);
                        let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                        for line in head.lines() {
                            let lower = line.to_ascii_lowercase();
                            if let Some(v) = lower.strip_prefix("content-length:") {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                }
                if let Some(end) = head_end {
                    if buf.len() >= end + content_length {
                        break;
                    }
                }
            }
            let end = head_end.unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let first_line = head.lines().next().unwrap_or("").to_string();
            let auth = head
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("authorization:")
                        .map(|v| v.trim().to_string())
                })
                .unwrap_or_default();
            let _ = tx.send(format!("{first_line}\t{auth}"));

            let body = if first_line.contains("/api/v0/chat/create_pow_challenge") {
                r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"challenge":{"algorithm":"SHA3-256","challenge":"test-challenge-payload","salt":"test-salt","difficulty":2.0,"expire_at":1752000000,"signature":"test-signature","target_path":"/api/v0/file/upload_file"}}}}"#.to_string()
            } else if first_line.contains("/api/v0/file/upload_file") {
                if muted.iter().any(|t| auth.contains(t.as_str())) {
                    r#"{"code":0,"msg":"","data":{"biz_code":14,"biz_msg":"user is muted","biz_data":{"is_muted":1,"mute_until":1794738505}}}"#.to_string()
                } else {
                    r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"file-1"}}}"#.to_string()
                }
            } else {
                r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{}}}"#
                    .to_string()
            };
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 造一张 1x1 PNG。
fn write_tiny_png(home: &Path) -> PathBuf {
    let png = home.join("tiny.png");
    let bytes: Vec<u8> = (vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ])
    .into_iter()
    .collect();
    std::fs::write(&png, bytes).expect("write png");
    png
}

/// 轮换 + 自动切换：账号级失败后换下一个账号，失败账号被标记不可用并落盘。
#[test]
fn rotation_switches_account_on_upload_mute() {
    let home = isolated_home("rotation");
    let (base_url, rx) = spawn_mock(&["tok-acc-1", "tok-acc-2"]);
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-acc-1", "healthy", None),
            &account_json("acc-2", "tok-acc-2", "healthy", None),
        ],
    );
    let png = write_tiny_png(&home);

    let output = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("DEEPSEEK_UPLOAD_MIN_INTERVAL", "0")
        .arg("vision")
        .arg(&png)
        .arg("--json")
        .output()
        .expect("run vision");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !output.status.success(),
        "expected failure, got: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("muted") || stderr.contains("muted"),
        "expected a muted error: {stdout}{stderr}"
    );

    // 上传请求应依次使用两个账号（真轮换 + 账号级失败后自动切换）。
    let requests: Vec<String> = rx.try_iter().collect();
    let uploads: Vec<&String> = requests
        .iter()
        .filter(|r| r.contains("/api/v0/file/upload_file"))
        .collect();
    assert_eq!(
        uploads.len(),
        2,
        "expected one upload per account: {requests:?}"
    );
    assert!(
        uploads[0].contains("tok-acc-1"),
        "unexpected first: {requests:?}"
    );
    assert!(
        uploads[1].contains("tok-acc-2"),
        "unexpected second: {requests:?}"
    );

    // 失败账号标记为 muted 并落盘（重启后不再重复撞同一个坏账号）。
    let pool_raw = std::fs::read_to_string(home.join(".deepseek-visionary/accounts.json")).unwrap();
    let pool: serde_json::Value = serde_json::from_str(&pool_raw).expect("parse pool");
    let muted = pool["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .filter(|a| a["status"] == "muted")
        .count();
    assert_eq!(muted, 2, "{pool_raw}");
}

/// 跳过静音：池中已标记 muted 的账号不会被选中。
#[test]
fn muted_account_is_skipped() {
    let home = isolated_home("skip-muted");
    let (base_url, rx) = spawn_mock(&["tok-acc-2"]);
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-acc-1", "muted", Some(1794738505)),
            &account_json("acc-2", "tok-acc-2", "healthy", None),
        ],
    );
    let png = write_tiny_png(&home);

    let output = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("DEEPSEEK_UPLOAD_MIN_INTERVAL", "0")
        .arg("vision")
        .arg(&png)
        .arg("--json")
        .output()
        .expect("run vision");
    assert!(!output.status.success());

    let requests: Vec<String> = rx.try_iter().collect();
    let uploads: Vec<&String> = requests
        .iter()
        .filter(|r| r.contains("/api/v0/file/upload_file"))
        .collect();
    assert_eq!(
        uploads.len(),
        1,
        "muted account must be skipped: {requests:?}"
    );
    assert!(
        uploads[0].contains("tok-acc-2"),
        "the muted account must not be used: {requests:?}"
    );
}

/// 全不可用：池中所有账号都不可用 → 明确报错，且不发起任何上传。
#[test]
fn all_accounts_unusable_reports_clear_error() {
    let home = isolated_home("all-unusable");
    let (base_url, rx) = spawn_mock(&["tok-acc-1", "tok-acc-2"]);
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-acc-1", "muted", Some(1794738505)),
            &account_json("acc-2", "tok-acc-2", "expired", None),
        ],
    );
    let png = write_tiny_png(&home);

    let output = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .env("DEEPSEEK_UPLOAD_MIN_INTERVAL", "0")
        .arg("vision")
        .arg(&png)
        .arg("--json")
        .output()
        .expect("run vision");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(!output.status.success());
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.to_lowercase().contains("no usable account") || combined.contains("不可用"),
        "expected a clear no-usable-account error: {combined}"
    );

    let requests: Vec<String> = rx.try_iter().collect();
    assert!(
        !requests
            .iter()
            .any(|r| r.contains("/api/v0/file/upload_file")),
        "no upload may be attempted: {requests:?}"
    );
}

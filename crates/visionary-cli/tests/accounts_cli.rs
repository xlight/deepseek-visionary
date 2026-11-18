//! `accounts` 子命令组的二进制级集成测试（CLI 集成测试）。
//!
//! 覆盖：list / status / remove / use / use --clear / migrate、--json 形状、
//! 退出码、以及「池未启用」与「零可用账号」的区分提示。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_visionary-cli")
}

/// 隔离用户目录：放在工作区 target 下（保证子进程可写）。
fn isolated_home(tag: &str) -> PathBuf {
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(".accounts-cli-test")
        .join(tag);
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create isolated home");
    home
}

fn write_pool(home: &Path, accounts: &[&str]) {
    let dir = home.join(".deepseek-visionary");
    std::fs::create_dir_all(&dir).expect("create data dir");
    let body = format!(
        "{{\"accounts\":[{}],\"cursor\":0,\"strategy\":\"rotate\",\"pinned\":null,\"removed\":[]}}",
        accounts.join(",")
    );
    std::fs::write(dir.join("accounts.json"), body).expect("write pool");
}

fn account_json(id: &str, token: &str, status: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"label\":\"{id}@example.com\",\"user_token\":\"{token}\",\"smid_v2\":\"\",\"cf_clearance\":\"\",\"status\":\"{status}\",\"mute_until\":null,\"last_used\":null}}"
    )
}

/// 假探针服务：返回固定 body。
fn spawn_probe(body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let addr = listener.local_addr().expect("probe addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
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

const PROBE_OK: &str = r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"acc-migrated","email":"m@example.com","chat":{"is_muted":0,"mute_until":0,"msg":null}}}}"#;

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .args(args)
        .output()
        .expect("run cli")
}

#[test]
fn list_json_reports_empty_pool() {
    let home = isolated_home("list-empty");
    let out = run(&home, &["accounts", "list", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["enabled"], serde_json::Value::Bool(false));
    assert_eq!(v["total"], serde_json::json!(0));
    assert_eq!(v["accounts"].as_array().unwrap().len(), 0);
}

#[test]
fn list_json_reports_accounts_without_tokens() {
    let home = isolated_home("list-pool");
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-secret-1", "healthy"),
            &account_json("acc-2", "tok-secret-2", "muted"),
        ],
    );
    let out = run(&home, &["accounts", "list", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("tok-secret"),
        "tokens must not leak: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["enabled"], serde_json::Value::Bool(true));
    assert_eq!(v["total"], serde_json::json!(2));
    assert_eq!(v["healthy"], serde_json::json!(1));
    assert_eq!(v["muted"], serde_json::json!(1));
    assert_eq!(v["accounts"][0]["id"], "acc-1");
}

#[test]
fn list_text_suggests_migrate_when_pool_disabled_but_config_exists() {
    let home = isolated_home("list-migrate-hint");
    let dir = home.join(".deepseek-visionary");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        r#"{"user_token":"tok-legacy","smid_v2":"","cf_clearance":""}"#,
    )
    .unwrap();
    let out = run(&home, &["accounts", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("not enabled"),
        "expected the pool-disabled notice: {stdout}"
    );
    assert!(
        stdout.contains("accounts migrate"),
        "expected the migrate hint: {stdout}"
    );
}

#[test]
fn remove_drops_account_and_leaves_tombstone() {
    let home = isolated_home("remove");
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-1", "healthy"),
            &account_json("acc-2", "tok-2", "healthy"),
        ],
    );
    let out = run(&home, &["accounts", "remove", "--json", "acc-1"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["removed"]["id"], "acc-1");
    assert_eq!(v["pool"]["total"], serde_json::json!(1));

    let pool_raw = std::fs::read_to_string(home.join(".deepseek-visionary/accounts.json")).unwrap();
    let pool: serde_json::Value = serde_json::from_str(&pool_raw).unwrap();
    assert_eq!(pool["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(pool["removed"][0], "acc-1");
}

#[test]
fn remove_unknown_account_exits_non_zero() {
    let home = isolated_home("remove-unknown");
    write_pool(&home, &[&account_json("acc-1", "tok-1", "healthy")]);
    let out = run(&home, &["accounts", "remove", "nope"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("No such account"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn use_pins_and_clear_unpins() {
    let home = isolated_home("use");
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-1", "healthy"),
            &account_json("acc-2", "tok-2", "healthy"),
        ],
    );
    let out = run(&home, &["accounts", "use", "--json", "acc-2"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["pinned"], "acc-2");
    assert_eq!(v["current"], "acc-2");

    let out = run(&home, &["accounts", "use", "--json", "--clear"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["pinned"], serde_json::Value::Null);
}

#[test]
fn status_probes_accounts_and_reports_counts() {
    let home = isolated_home("status");
    let (base_url, _rx) = spawn_probe(PROBE_OK);
    write_pool(
        &home,
        &[
            &account_json("acc-1", "tok-1", "healthy"),
            &account_json("acc-2", "tok-2", "healthy"),
        ],
    );
    let out = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .args(["accounts", "status", "--json"])
        .output()
        .expect("run status");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["total"], serde_json::json!(2));
    assert_eq!(v["healthy"], serde_json::json!(2));
}

#[test]
fn migrate_imports_legacy_credentials_idempotently() {
    let home = isolated_home("migrate");
    let (base_url, _rx) = spawn_probe(PROBE_OK);
    let dir = home.join(".deepseek-visionary");
    std::fs::create_dir_all(&dir).unwrap();
    let compat = r#"{"user_token":"tok-legacy","smid_v2":"","cf_clearance":""}"#;
    std::fs::write(dir.join("config.json"), compat).unwrap();

    let out = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .args(["accounts", "migrate", "--json"])
        .output()
        .expect("run migrate");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["imported"], serde_json::json!(1));
    assert_eq!(v["pool"]["total"], serde_json::json!(1));
    // 非破坏：config.json 原样保留
    assert_eq!(
        std::fs::read_to_string(dir.join("config.json")).unwrap(),
        compat
    );

    // 幂等：再跑一次不新增
    let out = Command::new(bin())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("DEEPSEEK_BASE_URL", &base_url)
        .args(["accounts", "migrate", "--json"])
        .output()
        .expect("run migrate again");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["imported"], serde_json::json!(0));
    assert_eq!(v["skipped"], serde_json::json!(1));
    assert_eq!(v["pool"]["total"], serde_json::json!(1));
}

#[test]
fn zero_usable_accounts_reports_recovery_paths() {
    let home = isolated_home("zero-usable");
    write_pool(&home, &[&account_json("acc-1", "tok-1", "expired")]);
    let out = run(&home, &["accounts", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("expired"),
        "expected the expired marker: {stdout}"
    );
}

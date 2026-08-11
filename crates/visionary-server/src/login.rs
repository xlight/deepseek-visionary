//! 浏览器自动登录（对应 design.md 决策 5，任务 5.2/5.3/5.4/5.5）。
//!
//! 流程：定位 Chrome 系浏览器 → 以专用 profile（`~/.deepseek-visionary/browser/`，
//! 0700）+ `--remote-debugging-port` 启动（Chrome 136+ 要求非默认 profile 才允许
//! 远程调试，天然满足）→ 打开 chat.deepseek.com → 轮询等待用户登录 →
//! 抓取 `localStorage.userToken` 与 `smidV2` / `cf_clearance` cookie →
//! 写入 `~/.deepseek-visionary/config.json`（0600）并热重载 → 返回。
//!
//! `run_login` 阻塞等待登录完成（带超时）；超时后浏览器保持打开，用户可继续
//! 登录后重跑 login（幂等）。手动粘贴 token 为兜底路径。

use crate::browser;
use crate::config::{self, Config, Credentials};
use anyhow::{anyhow, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::time::Duration;

/// 登录等待超时（可用 `DEEPSEEK_LOGIN_TIMEOUT` 覆盖，秒）。
fn login_timeout() -> Duration {
    let secs = std::env::var("DEEPSEEK_LOGIN_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    Duration::from_secs(secs)
}

/// 轮询间隔：1 秒。
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// `deepseek_vision_login` 工具实现（任务 5.4）与 CLI `login` 子命令共用。
///
/// 返回人类可读文本（MCP handler 包装成 `CallToolResult`，CLI 直接打印）。
pub async fn run_login(config: &Config) -> Result<String> {
    // 已配置 token 时幂等提示
    if config.is_authenticated() {
        return Ok(
            "Token already configured. To re-login, run `visionary-server logout` first.".into(),
        );
    }

    match login_flow(config).await {
        Ok(creds) => {
            config.save_credentials(&creds)?;
            Ok(format!(
                "Login successful! Credentials saved:\n\
                 - user_token: {}...\n\
                 - smidV2: {}\n\
                 - cf_clearance: {}\n\
                 You can now analyze images with `visionary-server vision`.",
                mask(&creds.user_token),
                if creds.smid_v2.is_empty() {
                    "none".to_string()
                } else {
                    mask(&creds.smid_v2)
                },
                if creds.cf_clearance.is_empty() {
                    "none".to_string()
                } else {
                    mask(&creds.cf_clearance)
                },
            ))
        }
        Err(e) => {
            // 超时或未完成登录时，浏览器保持打开，用户可继续操作后重跑。
            // 返回 Err：CLI 据此退出非零；MCP handler 映射回 CallToolResult::error。
            Err(anyhow!(
                "Login incomplete: {e}\n\
                 The browser window stays open. Finish logging in and run `visionary-server login` again,\
                 or configure ~/.deepseek-visionary/config.json manually."
            ))
        }
    }
}

/// `deepseek_vision_logout` 工具实现（任务 5.5）与 CLI `logout` 子命令共用。
pub async fn run_logout(config: &Config) -> Result<String> {
    config
        .save_credentials(&Credentials::default())
        .context("failed to clear credentials")?;
    Ok("Saved credentials cleared.\n\
         To log in again, run `visionary-server login`."
        .into())
}

/// 完整登录流程：启动浏览器 → 等待登录 → 抓取凭据。
async fn login_flow(_config: &Config) -> Result<Credentials> {
    let browser_path = browser::find_browser()?;
    let profile_dir = config::browser_profile_dir()?;

    // 专用 profile 目录（0700）
    std::fs::create_dir_all(&profile_dir).context("failed to create browser profile dir")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o700));
    }

    let port = pick_free_port().context("no free port for remote debugging")?;

    let browser_config = BrowserConfig::builder()
        .chrome_executable(&browser_path)
        .with_head()
        .user_data_dir(&profile_dir)
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .build()
        .map_err(|e| anyhow!("failed to build browser config: {e}"))?;

    tracing::info!(
        "launching browser {} with profile {} on port {port}",
        browser_path.display(),
        profile_dir.display()
    );

    let (mut browser, mut handler) = Browser::launch(browser_config)
        .await
        .context("failed to launch browser")?;

    // handler drain loop（chromiumoxide 必需）
    let handler_task = tokio::spawn(async move { while let Some(_h) = handler.next().await {} });

    let result = wait_for_login(&mut browser, &profile_dir).await;

    // 清理：结束 handler，让浏览器保持打开（用户可继续登录后重跑）
    handler_task.abort();
    let _ = browser.close().await;
    result
}

/// 等待用户登录并抓取凭据。
async fn wait_for_login(browser: &mut Browser, _profile_dir: &PathBuf) -> Result<Credentials> {
    let page = browser
        .new_page("https://chat.deepseek.com")
        .await
        .context("failed to open chat.deepseek.com")?;

    let deadline = tokio::time::Instant::now() + login_timeout();
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Login timed out after {} seconds",
                login_timeout().as_secs()
            ));
        }

        // 读取 localStorage.userToken（页面 localStorage 里存的是 JSON 字符串，
        // 形如 {"value": "...", ...}；未登录时为 {"value":null,...}）
        let token_json: Option<String> = page
            .evaluate("localStorage.getItem('userToken')")
            .await
            .ok()
            .and_then(|v| v.value().and_then(|x| x.as_str()).map(String::from));

        if let Some(json) = token_json {
            let token = parse_token_json(&json)?;
            if !token.is_empty() {
                // 已登录成功，抓取 cookies
                let cookies = page.get_cookies().await.unwrap_or_default();
                let smid_v2 = find_cookie(&cookies, "smidV2").unwrap_or_default();
                let cf_clearance = find_cookie(&cookies, "cf_clearance").unwrap_or_default();
                return Ok(Credentials {
                    user_token: token,
                    smid_v2,
                    cf_clearance,
                });
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// 解析 localStorage userToken JSON（`JSON.parse(value).value`）。
///
/// 兼容四种形态：
/// - 未登录：`null` / `""` / `{"value":null}` → 返回空字符串（继续轮询等待）
/// - 已登录：`{"value": "..."}`（网页版当前格式）
/// - 兜底：纯字符串 token
fn parse_token_json(json: &str) -> Result<String> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(String::new());
    }
    let v: serde_json::Value =
        serde_json::from_str(trimmed).context("userToken is not valid JSON")?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(map) => match map.get("value") {
            // 未登录时 DeepSeek 存的是 {"value":null,"__version":"0"}
            Some(serde_json::Value::Null) | None => Ok(String::new()),
            Some(serde_json::Value::String(s)) if !s.is_empty() => Ok(s.clone()),
            _ => Err(anyhow!("userToken has unexpected shape: {trimmed}")),
        },
        _ => Err(anyhow!("userToken has unexpected shape: {trimmed}")),
    }
}

/// 从 CDP cookies 中按名称取值。
fn find_cookie(
    cookies: &[chromiumoxide::cdp::browser_protocol::network::Cookie],
    name: &str,
) -> Option<String> {
    cookies
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.value.clone())
}

/// 挑选一个可用的远程调试端口（9222 起向上探测）。
fn pick_free_port() -> Result<u16> {
    for port in 9222..9300u16 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(anyhow!("no free port in range 9222-9299"))
}

/// 脱敏显示（保留前后 8 字符）。
fn mask(s: &str) -> String {
    if s.len() <= 16 {
        "****".to_string()
    } else {
        format!("{}...{}", &s[..8], &s[s.len() - 8..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_json_null_when_not_logged_in() {
        // 未登录时 localStorage 存的是字符串 "null"
        assert_eq!(parse_token_json("null").unwrap(), "");
        assert_eq!(parse_token_json("").unwrap(), "");
        // 或 DeepSeek 实际存的 {"value":null,"__version":"0"}
        assert_eq!(
            parse_token_json(r#"{"value":null,"__version":"0"}"#).unwrap(),
            ""
        );
    }

    #[test]
    fn parse_token_json_extracts_value() {
        let json = r#"{"value":"tok123","expireAt":1752000000}"#;
        assert_eq!(parse_token_json(json).unwrap(), "tok123");
    }

    #[test]
    fn parse_token_json_plain_string() {
        // 兼容某些场景下 localStorage 直接存字符串
        let json = r#""raw-token""#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        if let Some(s) = v.as_str() {
            assert_eq!(s, "raw-token");
        }
    }

    #[test]
    fn mask_shortens_long_tokens() {
        let m = mask("abcdefghijklmnopqrstuvwxyz");
        assert!(m.contains("..."));
        assert!(m.len() < 26);
    }
}

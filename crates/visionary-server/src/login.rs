//! 浏览器自动登录（对应 design.md 决策 5，任务 5.2/5.3/5.4/5.5）。
//!
//! 流程：定位 Chrome 系浏览器 → 以专用 profile（`~/.deepseek-visionary/browser/`，
//! 0700）+ `--remote-debugging-port` 启动（Chrome 136+ 要求非默认 profile 才允许
//! 远程调试，天然满足）→ 打开 chat.deepseek.com → 轮询等待用户登录 →
//! 抓取 `localStorage.userToken` 与 `smidV2` / `cf_clearance` cookie →
//! 写入 `~/.deepseek-visionary/config.json`（0600）并热重载 → 返回。
//!
//! 反自动化检测：chromiumoxide 默认追加 `--enable-automation`（`navigator.webdriver=true`），
//! DeepSeek 登录页的 hCaptcha 会据此判定为机器人、人机验证永远无法通过。因此启动时追加
//! `--disable-blink-features=AutomationControlled`，并在导航前先建空白页调用
//! `enable_stealth_mode_with_agent` 注入隐藏 webdriver / permissions / plugins / WebGL
//! 指纹的初始化脚本（对目标页及 hCaptcha iframe 均生效），再跳转 chat.deepseek.com。
//!
//! 登录页语言：默认跟随系统 locale（`LC_ALL` / `LANG`，自适应），可用
//! `DEEPSEEK_LOGIN_LANG` 环境变量手动切换（如 `zh-CN` / `en`）。chromiumoxide
//! 默认 `--lang=en_US` 会把页面固定为英文版，故按解析结果追加 `--lang` / `--accept-lang`。
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

/// 登录页语言解析（自适应 + 手动切换）。
///
/// 优先级：
/// 1. `DEEPSEEK_LOGIN_LANG` 环境变量显式指定（如 `zh-CN` / `en` / `ja`）——手动切换；
/// 2. 系统 `LC_ALL` / `LC_MESSAGES` / `LANG`（如 `zh_CN.UTF-8` → `zh-CN`）——自适应；
/// 3. 均缺失时默认 `zh-CN`（DeepSeek 为中文产品，目标用户以中文为主）。
///
/// 页面语言由浏览器 `Accept-Language` 决定（chromiumoxide 默认 `--lang=en_US`
/// 会把登录页固定成英文版），因此按此结果追加 `--lang` / `--accept-lang` 启动参数。
fn login_lang() -> String {
    if let Ok(v) = std::env::var("DEEPSEEK_LOGIN_LANG") {
        let v = v.trim();
        if !v.is_empty() {
            return normalize_lang(v);
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if let Some(l) = derive_lang_from_locale(&v) {
                return l;
            }
        }
    }
    "zh-CN".to_string()
}

/// 归一化用户显式指定的语言标签（`zh-cn` / `zh_CN` → `zh-CN`）。
fn normalize_lang(lang: &str) -> String {
    let parts: Vec<&str> = lang.trim().split(['-', '_']).collect();
    match parts.as_slice() {
        [l] => l.to_ascii_lowercase(),
        [l, r] => format!("{}-{}", l.to_ascii_lowercase(), r.to_ascii_uppercase()),
        _ => parts[0].to_ascii_lowercase(),
    }
}

/// 从系统 locale（`zh_CN.UTF-8` / `en_US` / `C`）推导语言标签；无法识别返回 None。
fn derive_lang_from_locale(locale: &str) -> Option<String> {
    let loc = locale.trim();
    if loc.is_empty() || loc == "C" || loc == "POSIX" {
        return None;
    }
    let lang_part = loc.split('.').next().unwrap_or(loc);
    Some(normalize_lang(lang_part))
}

/// 构造 `--accept-lang` 值：主语言优先，英文兜底。
fn accept_lang(lang: &str) -> String {
    let base = lang.split('-').next().unwrap_or(lang).to_ascii_lowercase();
    match base.as_str() {
        "zh" => "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
        "en" => "en-US,en;q=0.9".to_string(),
        "ja" => "ja-JP,ja;q=0.9,en;q=0.8".to_string(),
        _ => format!("{lang},{base};q=0.9,en;q=0.8"),
    }
}

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
            // 超时或未完成登录时，浏览器进程会被关闭，但专用 profile 中的会话
            // 保留：用户解决验证码/完成登录后重跑 login 即可秒级抓取凭据。
            // 返回 Err：CLI 据此退出非零；MCP handler 映射回 CallToolResult::error。
            Err(anyhow!(
                "Login incomplete: {e}\n\
                 The browser window was closed, but the login session in the profile is preserved.\n\
                 Solve the captcha / finish logging in, then run `visionary-server login` again,\
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

    // 登录页语言：默认跟随系统 locale（自适应），可用 DEEPSEEK_LOGIN_LANG 手动切换。
    // 不传 --accept-lang 时 chromiumoxide 默认 --lang=en_US 会把页面固定成英文版。
    let lang = login_lang();
    let accept_lang = accept_lang(&lang);

    let browser_config = BrowserConfig::builder()
        .chrome_executable(&browser_path)
        .with_head()
        .user_data_dir(&profile_dir)
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 隐藏 automation 指纹：chromiumoxide 默认带 --enable-automation，
        // 登录页 hCaptcha 会据此拒绝通过人机验证（配合下方 stealth 脚本双保险）
        .arg("--disable-blink-features=AutomationControlled")
        // 登录页语言（zh-CN / en / …）：--accept-lang 决定页面渲染语言，
        // --lang 覆盖 chromiumoxide 的默认 en_US UI locale
        .arg(format!("--lang={lang}"))
        .arg(format!("--accept-lang={accept_lang}"))
        .build()
        .map_err(|e| anyhow!("failed to build browser config: {e}"))?;

    tracing::info!(
        "launching browser {} with profile {} on port {port} (lang {lang}, accept-lang {accept_lang})",
        browser_path.display(),
        profile_dir.display()
    );

    let (mut browser, mut handler) = Browser::launch(browser_config)
        .await
        .context("failed to launch browser")?;

    // handler drain loop（chromiumoxide 必需）
    let handler_task = tokio::spawn(async move { while let Some(_h) = handler.next().await {} });

    let result = wait_for_login(&mut browser, &profile_dir).await;

    // 清理：浏览器进程随后会被 chromiumoxide 关闭，但专用 profile 中的
    // 登录会话保留，用户重跑 login 即可续接（见 run_login 错误提示）
    handler_task.abort();
    let _ = browser.close().await;
    result
}

/// 等待用户登录并抓取凭据。
async fn wait_for_login(browser: &mut Browser, _profile_dir: &PathBuf) -> Result<Credentials> {
    // 先创建空白页并注入反自动化脚本（隐藏 navigator.webdriver 等指纹），
    // 再导航到 chat.deepseek.com —— 否则登录页的 hCaptcha 检测到自动化
    // 浏览器后，人机验证始终无法通过（"过不了 capture"）。
    let page = browser
        .new_page("about:blank")
        .await
        .context("failed to create browser page")?;

    // 读取真实 UA 并原样保留：伪装成别的系统反而更容易被风控识别
    let real_ua = page
        .evaluate("navigator.userAgent")
        .await
        .ok()
        .and_then(|v| v.value().and_then(|x| x.as_str()).map(String::from))
        .unwrap_or_default();

    page.enable_stealth_mode_with_agent(&real_ua)
        .await
        .context("failed to enable anti-detection stealth mode")?;

    page.goto("https://chat.deepseek.com")
        .await
        .context("failed to open chat.deepseek.com")?;

    let deadline = tokio::time::Instant::now() + login_timeout();
    let mut captcha_hinted = false;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Login timed out after {} seconds",
                login_timeout().as_secs()
            ));
        }

        // 检测到人机验证（hCaptcha）时提示一次，引导用户在浏览器窗口手动完成
        if !captcha_hinted {
            let has_captcha = page
                .evaluate("document.querySelector('iframe[src*=\"hcaptcha\"]') !== null")
                .await
                .ok()
                .and_then(|v| v.value().and_then(|x| x.as_bool()))
                .unwrap_or(false);
            if has_captcha {
                tracing::info!(
                    "detected hCaptcha on the DeepSeek sign-in page; \
                     please solve it manually in the browser window"
                );
                captcha_hinted = true;
            }
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

    #[test]
    fn normalize_lang_variants() {
        assert_eq!(normalize_lang("zh-CN"), "zh-CN");
        assert_eq!(normalize_lang("zh_CN"), "zh-CN");
        assert_eq!(normalize_lang("zh-cn"), "zh-CN");
        assert_eq!(normalize_lang("en"), "en");
        assert_eq!(normalize_lang("ja-JP"), "ja-JP");
    }

    #[test]
    fn derive_lang_from_locale_variants() {
        assert_eq!(derive_lang_from_locale("zh_CN.UTF-8").unwrap(), "zh-CN");
        assert_eq!(derive_lang_from_locale("en_US").unwrap(), "en-US");
        assert_eq!(derive_lang_from_locale("ja_JP.UTF-8").unwrap(), "ja-JP");
        assert_eq!(derive_lang_from_locale("C"), None);
        assert_eq!(derive_lang_from_locale("POSIX"), None);
        assert_eq!(derive_lang_from_locale(""), None);
    }

    #[test]
    fn accept_lang_builds_priority_list() {
        assert_eq!(accept_lang("zh-CN"), "zh-CN,zh;q=0.9,en;q=0.8");
        assert_eq!(accept_lang("en"), "en-US,en;q=0.9");
        assert_eq!(accept_lang("ja-JP"), "ja-JP,ja;q=0.9,en;q=0.8");
    }
}

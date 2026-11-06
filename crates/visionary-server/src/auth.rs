//! 鉴权头与 cookies 构造（对照 Python 版 `auth.py`）。

use crate::config::Config;
use anyhow::{Context, Result};

/// 与 Python `AuthManager` 对齐：构造 DeepSeek API 请求头。
pub struct AuthManager<'a> {
    config: &'a Config,
}

impl<'a> AuthManager<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// 基础请求头（对应 Python `get_headers`）。
    pub fn headers(&self) -> Vec<(String, String)> {
        let token = self.config.credentials().user_token;
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            (
                "User-Agent".into(),
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"
                    .into(),
            ),
            (
                "Accept".into(),
                "application/json, text/event-stream, text/plain, */*".into(),
            ),
            ("Accept-Language".into(), "zh-CN,zh;q=0.9,en;q=0.8".into()),
            ("X-App-Version".into(), self.config.app_version.clone()),
            ("X-Client-Version".into(), "1.0.0-always".into()),
            ("X-Client-Locale".into(), "zh-CN".into()),
            ("X-Client-Platform".into(), "web".into()),
        ]
    }

    /// 请求 cookies（对应 Python `get_cookies`：仅 cf_clearance）。
    pub fn cookies(&self) -> Vec<(String, String)> {
        let c = self.config.credentials();
        let mut out = Vec::new();
        if !c.cf_clearance.is_empty() {
            out.push(("cf_clearance".into(), c.cf_clearance));
        }
        out
    }

    /// 校验 token 已配置（对应 Python `validate`：仅查非空，
    /// 真实校验见 `deepseek_vision_status` 工具——任务 4.3）。
    pub fn validate(&self) -> bool {
        self.config.is_authenticated()
    }
}

/// 真实 token 校验探针：调用流水线首个需鉴权端点 create_pow_challenge。
/// 401 即 token 失效。`deepseek_vision_status` 工具与 `doctor` 子命令共用。
pub async fn probe_token(config: &Config) -> Result<()> {
    let client =
        crate::client::ApiClient::new(config.clone()).context("init api client for probe")?;
    crate::upload::create_pow_challenge(&client, "/api/v0/chat/completion").await?;
    Ok(())
}

/// token 自动续期开关（`DEEPSEEK_AUTO_REFRESH=0` 关闭，默认开启）。
///
/// 关闭后 token 失效会直接把原始错误抛给调用方，由用户手动
/// `visionary-server logout && visionary-server login` 处理。
pub fn auto_refresh_enabled() -> bool {
    match std::env::var("DEEPSEEK_AUTO_REFRESH") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    }
}

/// token 失效时自动续期：重跑浏览器登录流程，用持久 profile 里的会话换一个新的
/// `userToken` 并覆盖保存。
///
/// DeepSeek 网页端每次加载页面都会轮换 `localStorage.userToken` 并作废旧 token，
/// CLI 存的只是快照——不续期的话，用一会儿就会撞上
/// `biz_code=40003 Authorization Failed (invalid token)`。
///
/// 会短暂拉起一次浏览器窗口（headful）：新 token 只能由网页端签发，没有纯 HTTP 的
/// 换取途径。持久 profile 已登录时通常几秒内即可拿到新 token。
pub async fn refresh_token(config: &Config) -> Result<()> {
    if !config.auto_refresh {
        anyhow::bail!(
            "token expired and auto-refresh is disabled (DEEPSEEK_AUTO_REFRESH=0); run `visionary-server logout && visionary-server login` to refresh it manually"
        );
    }
    let timeout = crate::login::refresh_timeout();
    tracing::warn!(
        "token rejected by DeepSeek; refreshing credentials via browser login (timeout {}s)",
        timeout.as_secs()
    );
    let _creds = crate::login::refresh_credentials(config, timeout).await?;
    tracing::info!("credentials refreshed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_config() -> Config {
        let mut cfg = Config::load().unwrap();
        cfg.credentials = Arc::new(std::sync::RwLock::new(crate::config::Credentials {
            user_token: "tok".into(),
            smid_v2: "smid".into(),
            cf_clearance: "cf".into(),
        }));
        cfg
    }

    #[test]
    fn headers_contain_bearer_and_app_version() {
        let cfg = test_config();
        let auth = AuthManager::new(&cfg);
        let h = auth.headers();
        assert!(h
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer tok"));
        assert!(h.iter().any(|(k, v)| k == "X-App-Version" && v == "2.0.0"));
    }

    #[test]
    fn cookies_only_cf_clearance() {
        let cfg = test_config();
        let auth = AuthManager::new(&cfg);
        let c = auth.cookies();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0], ("cf_clearance".to_string(), "cf".to_string()));
    }

    #[test]
    fn auto_refresh_switch_reads_env() {
        let old = std::env::var("DEEPSEEK_AUTO_REFRESH").ok();
        std::env::set_var("DEEPSEEK_AUTO_REFRESH", "0");
        assert!(!auto_refresh_enabled());
        std::env::set_var("DEEPSEEK_AUTO_REFRESH", "false");
        assert!(!auto_refresh_enabled());
        std::env::set_var("DEEPSEEK_AUTO_REFRESH", "1");
        assert!(auto_refresh_enabled());
        match old {
            Some(v) => std::env::set_var("DEEPSEEK_AUTO_REFRESH", v),
            None => std::env::remove_var("DEEPSEEK_AUTO_REFRESH"),
        }
    }
}

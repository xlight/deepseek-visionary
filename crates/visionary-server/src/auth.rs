//! 鉴权头与 cookies 构造（对照 Python 版 `auth.py`）。

use crate::config::Config;

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
}

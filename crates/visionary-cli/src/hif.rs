//! HIF（High-Integrity Framework）签名 token 管理（对照 Python 版 `hif_auth.py`）。
//!
//! 获取 `x-hif-leim` / `x-hif-dliq` 头值，token 有效期 600s，缓存至 80% 时刷新。
//!
//! # dliq 为何是可选的
//!
//! `hif-dliq.deepseek.com` 域名仍在（CNAME 指向华为云 WAF），但服务端已撤掉它的
//! **A 记录（IPv4）**，只剩 AAAA 记录。DNS 对「域名有效但无该类型数据」返回
//! `WSANO_DATA`（Windows 错误 11004），表现为：
//! `dns error: 请求的名称有效，但是找不到请求的类型的数据`。
//! 在没有可用公网 IPv6 的网络里因此连不通。
//!
//! 实测 completion 仅需 `x-hif-leim` 即可通过（HTTP 200），故 dliq 为**可选**：
//! 获取失败仅记 debug（默认日志级别 info 下不输出），不阻塞流水线。
//! （历史兜底：dliq 仅 AAAA 记录时解析 leim 的 IPv4 并用 resolve 覆盖——已弃用。）

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const LEIM_URL: &str = "https://hif-leim.deepseek.com/query";
const DLIQ_URL: &str = "https://hif-dliq.deepseek.com/query";
const TTL: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct CachedTokens {
    leim: String,
    dliq: Option<String>,
    expires_at: Instant,
}

pub struct HifAuth {
    config: Config,
    /// HIF 签名与账号 token 绑定，因此缓存按账号 id 分桶。
    cache: Mutex<HashMap<String, CachedTokens>>,
}

impl HifAuth {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 获取指定账号的 HIF 头（缓存未过期则复用；对应 Python `get_headers`）。
    pub async fn get_headers(&self, account_id: &str) -> Result<Vec<(String, String)>> {
        {
            let guard = self
                .cache
                .lock()
                .map_err(|e| anyhow!("hif cache lock poisoned: {e}"))?;
            if let Some(c) = guard.get(account_id) {
                if Instant::now() < c.expires_at {
                    let mut headers = vec![("x-hif-leim".into(), c.leim.clone())];
                    if let Some(d) = &c.dliq {
                        headers.push(("x-hif-dliq".into(), d.clone()));
                    }
                    return Ok(headers);
                }
            }
        }
        let (leim, dliq) = self.refresh(account_id).await?;
        {
            let mut guard = self
                .cache
                .lock()
                .map_err(|e| anyhow!("hif cache lock poisoned: {e}"))?;
            guard.insert(
                account_id.to_string(),
                CachedTokens {
                    leim: leim.clone(),
                    dliq: dliq.clone(),
                    expires_at: Instant::now() + TTL.mul_f32(0.8),
                },
            );
        }
        let mut headers = vec![("x-hif-leim".into(), leim)];
        if let Some(d) = dliq {
            headers.push(("x-hif-dliq".into(), d));
        }
        Ok(headers)
    }

    /// 作废指定账号缓存的 HIF token，不影响其它账号。
    ///
    /// DeepSeek 的 userToken 会被轮换，HIF token 是用旧 token 换来的，
    /// token 续期后必须作废缓存，否则签名与新身份不匹配。
    pub fn invalidate(&self, account_id: &str) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.remove(account_id);
        }
    }

    /// 作废全部账号的 HIF token（单账号模式续期时使用）。
    pub fn invalidate_all(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
        }
    }

    /// 刷新指定账号的 token（对应 Python `_refresh`）：leim 必需，dliq 可选。
    async fn refresh(&self, account_id: &str) -> Result<(String, Option<String>)> {
        // 签名必须使用该账号自己的 token，不能读取可能已被热替换的当前凭据。
        let token = self.config.credentials_for(account_id).user_token;
        let common = vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Origin".into(), "https://chat.deepseek.com".into()),
            ("Referer".into(), "https://chat.deepseek.com/".into()),
            (
                "User-Agent".into(),
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
                    .into(),
            ),
        ];

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("build hif http client")?;

        let leim = Self::fetch(&client, LEIM_URL, &common).await?;

        // dliq 可选：服务端已撤掉该域名的 A 记录（只剩 AAAA），无可用公网 IPv6
        // 时取不到；dliq 缺失不影响 completion，故只记 debug 不刷屏、不阻塞。
        let dliq = match Self::fetch(&client, DLIQ_URL, &common).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::debug!("HIF dliq fetch failed (optional, skipped): {e:#}");
                None
            }
        };

        match &dliq {
            Some(d) => tracing::info!(
                "HIF tokens refreshed (leim={}..., dliq={}...)",
                &leim[..leim.len().min(20)],
                &d[..d.len().min(20)]
            ),
            None => tracing::info!(
                "HIF token refreshed (leim={}..., dliq=skipped)",
                &leim[..leim.len().min(20)]
            ),
        }
        Ok((leim, dliq))
    }

    async fn fetch(
        client: &reqwest::Client,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<String> {
        let mut req = client.get(url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status != reqwest::StatusCode::OK {
            return Err(anyhow!("GET {url} returned HTTP {status}: {text}"));
        }
        let v: serde_json::Value =
            serde_json::from_str(&text).with_context(|| format!("parse {url} response"))?;
        v["data"]["biz_data"]["value"]
            .as_str()
            .map(String::from)
            .with_context(|| format!("{url}: missing data.biz_data.value"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            credentials: std::sync::Arc::new(std::sync::RwLock::new(crate::config::Credentials {
                user_token: "tok".into(),
                smid_v2: String::new(),
                cf_clearance: String::new(),
            })),
            pool: std::sync::Arc::new(std::sync::RwLock::new(Default::default())),
            base_url: "https://chat.deepseek.com".into(),
            poll_timeout: Duration::from_secs(60),
            poll_interval: Duration::from_secs(1),
            chat_timeout: Duration::from_secs(120),
            upload_timeout: Duration::from_secs(60),
            max_retries: 3,
            app_version: "test".into(),
            client_locale: "zh_CN".into(),
            model_type: None,
            upload_min_interval: Duration::from_secs(5),
            auto_refresh: true,
        }
    }

    fn cached_tokens(leim: &str) -> CachedTokens {
        CachedTokens {
            leim: leim.to_string(),
            dliq: None,
            expires_at: Instant::now() + TTL,
        }
    }

    /// 作废一个账号时，其它账号的 HIF 签名缓存必须保留。
    #[test]
    fn invalidate_only_removes_target_account() {
        let auth = HifAuth::new(test_config());
        {
            let mut cache = auth.cache.lock().expect("lock cache");
            cache.insert("account-a".to_string(), cached_tokens("leim-a"));
            cache.insert("account-b".to_string(), cached_tokens("leim-b"));
        }

        auth.invalidate("account-a");

        let cache = auth.cache.lock().expect("lock cache");
        assert!(!cache.contains_key("account-a"));
        assert_eq!(
            cache.get("account-b").map(|tokens| tokens.leim.as_str()),
            Some("leim-b")
        );
    }
}

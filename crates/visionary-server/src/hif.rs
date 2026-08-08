//! HIF（High-Integrity Framework）签名 token 管理（对照 Python 版 `hif_auth.py`）。
//!
//! 获取 `x-hif-leim` / `x-hif-dliq` 头值，token 有效期 600s，缓存至 80% 时刷新。
//! `hif-dliq.deepseek.com` 仅 AAAA 记录，IPv6 不通时兜底为：
//! 解析 `hif-leim` 的 IPv4（同一 CloudFront 分发），把 dliq 域名 resolve 到该地址
//! （SNI 仍为 dliq，对应 Python 版 patch anyio.connect_tcp 的行为）。

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const LEIM_URL: &str = "https://hif-leim.deepseek.com/query";
const DLIQ_URL: &str = "https://hif-dliq.deepseek.com/query";
const TTL: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct CachedTokens {
    leim: String,
    dliq: String,
    expires_at: Instant,
}

pub struct HifAuth {
    config: Config,
    cache: Mutex<Option<CachedTokens>>,
}

impl HifAuth {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            cache: Mutex::new(None),
        }
    }

    /// 获取 HIF 头（缓存未过期则复用；对应 Python `get_headers`）。
    pub async fn get_headers(&self) -> Result<Vec<(String, String)>> {
        {
            let guard = self
                .cache
                .lock()
                .map_err(|e| anyhow!("hif cache lock poisoned: {e}"))?;
            if let Some(c) = guard.as_ref() {
                if Instant::now() < c.expires_at {
                    return Ok(vec![
                        ("x-hif-leim".into(), c.leim.clone()),
                        ("x-hif-dliq".into(), c.dliq.clone()),
                    ]);
                }
            }
        }
        let (leim, dliq) = self.refresh().await?;
        {
            let mut guard = self
                .cache
                .lock()
                .map_err(|e| anyhow!("hif cache lock poisoned: {e}"))?;
            *guard = Some(CachedTokens {
                leim: leim.clone(),
                dliq: dliq.clone(),
                expires_at: Instant::now() + TTL.mul_f32(0.8),
            });
        }
        Ok(vec![
            ("x-hif-leim".into(), leim),
            ("x-hif-dliq".into(), dliq),
        ])
    }

    /// 刷新两个 token（对应 Python `_refresh`）。
    async fn refresh(&self) -> Result<(String, String)> {
        let token = self.config.credentials().user_token;
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

        // dliq 优先直连；失败（多为 IPv6 不通）时兜底：解析 leim 的 IPv4
        // 并把 dliq 域名 resolve 到该地址（SNI 仍为 hif-dliq.deepseek.com）。
        let dliq = match Self::fetch(&client, DLIQ_URL, &common).await {
            Ok(v) => v,
            Err(first_err) => match Self::fetch_with_dliq_ipv4_fallback(&common).await {
                Ok(v) => v,
                Err(fallback_err) => {
                    return Err(anyhow!(
                        "HIF dliq fetch failed (direct: {first_err:#}; fallback: {fallback_err:#})"
                    ));
                }
            },
        };

        tracing::info!(
            "HIF tokens refreshed (leim={}..., dliq={}...)",
            &leim[..leim.len().min(20)],
            &dliq[..dliq.len().min(20)]
        );
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

    /// dliq IPv4 兜底：解析 leim 的 IPv4，用 resolve 覆盖 dliq 域名重试。
    async fn fetch_with_dliq_ipv4_fallback(headers: &[(String, String)]) -> Result<String> {
        let leim_host = "hif-leim.deepseek.com";
        let mut addrs = tokio::net::lookup_host((leim_host, 443))
            .await
            .context("resolve hif-leim")?;
        let ipv4 = addrs
            .find(|a| a.is_ipv4())
            .context("hif-leim has no IPv4 address")?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .resolve("hif-dliq.deepseek.com", SocketAddr::new(ipv4.ip(), 443))
            .build()
            .context("build hif fallback http client")?;
        Self::fetch(&client, DLIQ_URL, headers).await
    }
}

//! HIF（High-Integrity Framework）签名 token 管理（对照 Python 版 `hif_auth.py`）。
//!
//! 获取 `x-hif-leim` / `x-hif-dliq` 头值，token 有效期 600s，缓存至 80% 时刷新。
//! `hif-dliq.deepseek.com` 已从服务端下线（当前环境 DNS 解析失败，Python 版同受其害）；
//! 实测 completion 仅需 `x-hif-leim` 即可通过（HTTP 200），故 dliq 改为**可选**：
//! 获取失败仅记录 warning，不阻塞流水线。
//! （历史兜底：dliq 仅 AAAA 记录时解析 leim 的 IPv4 并用 resolve 覆盖——已弃用。）

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
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
                    let mut headers = vec![("x-hif-leim".into(), c.leim.clone())];
                    if let Some(d) = &c.dliq {
                        headers.push(("x-hif-dliq".into(), d.clone()));
                    }
                    return Ok(headers);
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
        let mut headers = vec![("x-hif-leim".into(), leim)];
        if let Some(d) = dliq {
            headers.push(("x-hif-dliq".into(), d));
        }
        Ok(headers)
    }

    /// 刷新 token（对应 Python `_refresh`）：leim 必需，dliq 可选。
    async fn refresh(&self) -> Result<(String, Option<String>)> {
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

        // dliq 可选：域名已下线，仅尝试直连；失败只告警不阻塞。
        let dliq = match Self::fetch(&client, DLIQ_URL, &common).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("HIF dliq fetch failed (optional, skipped): {e:#}");
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

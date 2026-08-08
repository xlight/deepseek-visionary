//! 统一 HTTP 客户端封装。
//!
//! 对应 Python 各模块中 `httpx.AsyncClient` 的用法：
//! - 统一注入 base_url / 鉴权头 / cookies
//! - 超时与重试（对齐 Python：poll 60s / chat 120s / upload 60s / max_retries 3）
//! - JSON 响应统一解包 `biz_code` 业务错误

use crate::auth::AuthManager;
use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use reqwest::StatusCode;
use std::time::Duration;

pub struct ApiClient {
    pub http: reqwest::Client,
    pub config: Config,
}

/// DeepSeek API 的通用响应壳：`{ code/biz_code, msg, data: { biz_data } }`。
#[derive(Debug, serde::Deserialize)]
pub struct ApiEnvelope {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default, rename = "biz_code")]
    pub biz_code: Option<i64>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub data: Option<ApiData>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ApiData {
    #[serde(default)]
    pub biz_data: serde_json::Value,
}

impl ApiEnvelope {
    /// 校验业务码为 0，返回 `data.biz_data`。
    pub fn into_biz_data(self, context: &str) -> Result<serde_json::Value> {
        let biz = self.biz_code.or(self.code).unwrap_or(0);
        if biz != 0 {
            return Err(anyhow!(
                "{context} failed: biz_code={biz}, msg={}",
                self.msg.as_deref().unwrap_or("unknown error")
            ));
        }
        self.data
            .map(|d| d.biz_data)
            .context(format!("{context}: missing data.biz_data"))
    }
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(config.chat_timeout)
            .build()
            .context("failed to build reqwest client")?;
        Ok(Self { http, config })
    }

    fn base_headers(&self) -> Vec<(String, String)> {
        AuthManager::new(&self.config).headers()
    }

    fn base_cookies(&self) -> Vec<(String, String)> {
        AuthManager::new(&self.config).cookies()
    }

    /// 把 (String, String) 头列表应用到请求构建器。
    fn apply_headers(
        req: reqwest::RequestBuilder,
        headers: &[(String, String)],
    ) -> reqwest::RequestBuilder {
        headers
            .iter()
            .fold(req, |r, (k, v)| r.header(k.as_str(), v.as_str()))
    }

    /// 把 (String, String) cookie 列表应用到请求构建器。
    fn apply_cookies(
        req: reqwest::RequestBuilder,
        cookies: &[(String, String)],
    ) -> reqwest::RequestBuilder {
        cookies.iter().fold(req, |r, (k, v)| {
            r.header(reqwest::header::COOKIE, format!("{k}={v}"))
        })
    }

    /// GET 并返回 JSON 响应壳。
    pub async fn get_json(&self, path: &str, params: &[(&str, String)]) -> Result<ApiEnvelope> {
        let url = format!("{}{}", self.config.base_url, path);
        let req = Self::apply_cookies(
            Self::apply_headers(self.http.get(&url).query(params), &self.base_headers()),
            &self.base_cookies(),
        );
        let resp = req.send().await.with_context(|| format!("GET {path}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status != StatusCode::OK {
            return Err(anyhow!("GET {path} returned HTTP {status}: {text}"));
        }
        serde_json::from_str(&text).with_context(|| format!("parse GET {path} response"))
    }

    /// POST JSON body 并返回 JSON 响应壳（带重试）。
    pub async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<ApiEnvelope> {
        let url = format!("{}{}", self.config.base_url, path);
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..self.config.max_retries {
            let req = Self::apply_cookies(
                Self::apply_headers(
                    self.http.post(&url).timeout(timeout).json(body),
                    &self.base_headers(),
                ),
                &self.base_cookies(),
            );
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status != StatusCode::OK {
                        return Err(anyhow!("POST {path} returned HTTP {status}: {text}"));
                    }
                    return serde_json::from_str(&text)
                        .with_context(|| format!("parse POST {path} response"));
                }
                Err(e) => {
                    last_err = Some(anyhow!("POST {path} attempt {} failed: {e}", attempt + 1));
                    tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("POST {path} failed")))
    }

    /// POST multipart 文件上传（对应 Python upload 的 `files=`）。
    #[allow(clippy::too_many_arguments)]
    pub async fn post_multipart(
        &self,
        path: &str,
        field: &str,
        filename: &str,
        mime: &str,
        data: Vec<u8>,
        extra_headers: Vec<(String, String)>,
        timeout: Duration,
    ) -> Result<ApiEnvelope> {
        let url = format!("{}{}", self.config.base_url, path);
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str(mime)
            .context("invalid mime")?;
        let form = reqwest::multipart::Form::new().part(field.to_string(), part);

        let mut req = Self::apply_headers(
            self.http.post(&url).timeout(timeout).multipart(form),
            &self.base_headers(),
        );
        for (k, v) in extra_headers {
            req = req.header(&k, v);
        }
        let req = Self::apply_cookies(req, &self.base_cookies());
        let resp = req.send().await.with_context(|| format!("POST {path}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status == StatusCode::FORBIDDEN {
            return Err(anyhow!("POST {path} returned 403 after PoW: {text}"));
        }
        if status != StatusCode::OK {
            return Err(anyhow!("POST {path} returned HTTP {status}: {text}"));
        }
        serde_json::from_str(&text).with_context(|| format!("parse POST {path} response"))
    }
}

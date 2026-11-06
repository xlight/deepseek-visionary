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
    /// 业务码。线上响应把它放在 `data` 里（顶层 `code` 恒为 0），
    /// 例如上传被风控时 `data.biz_code=14` / `data.biz_msg="user is muted"`。
    #[serde(default)]
    pub biz_code: Option<i64>,
    /// 业务错误描述（对应 `data.biz_msg`）。
    #[serde(default)]
    pub biz_msg: Option<String>,
    #[serde(default)]
    pub biz_data: serde_json::Value,
}

impl ApiEnvelope {
    /// 校验业务码为 0，返回 `data.biz_data`。
    ///
    /// DeepSeek 的业务码有两个位置：顶层 `biz_code`/`code`，以及 `data.biz_code`。
    /// 线上响应实际把业务码放在 `data` 里（顶层 `code` 恒为 0），
    /// 只读顶层会漏判业务错误，最终报出 `file missing id` 这类难以定位的下游错误。
    pub fn into_biz_data(self, context: &str) -> Result<serde_json::Value> {
        let biz = self
            .biz_code
            .filter(|c| *c != 0)
            .or_else(|| {
                self.data
                    .as_ref()
                    .and_then(|d| d.biz_code)
                    .filter(|c| *c != 0)
            })
            .or(self.code)
            .unwrap_or(0);
        if biz != 0 {
            let msg = self
                .msg
                .as_deref()
                .filter(|m| !m.is_empty())
                .or_else(|| {
                    self.data
                        .as_ref()
                        .and_then(|d| d.biz_msg.as_deref())
                        .filter(|m| !m.is_empty())
                })
                .unwrap_or("unknown error");
            return Err(anyhow!("{context} failed: biz_code={biz}, msg={msg}"));
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
            apply_headers(self.http.get(&url).query(params), &self.base_headers())?,
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
                apply_headers(
                    self.http.post(&url).timeout(timeout).json(body),
                    &self.base_headers(),
                )?,
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

        let mut all_headers = self.base_headers();
        all_headers.extend(extra_headers);
        let req = apply_headers(
            self.http.post(&url).timeout(timeout).multipart(form),
            &all_headers,
        )?;
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

/// 把 (String, String) 头列表应用到请求构建器。
///
/// 这里刻意使用 `HeaderMap` 的 insert（**替换**）语义，而不是
/// `RequestBuilder::header` 的 append（**追加**）语义：
/// `reqwest` 的 `.json()` 已经自动设置了一个 `content-type: application/json`；
/// 若再追加一个同名头，请求就会带上**两个** content-type。DeepSeek 服务端严格
/// 校验该头，收到重复值时返回 HTTP 415
/// `Expected request with \`Content-Type: application/json\``。
pub(crate) fn apply_headers(
    req: reqwest::RequestBuilder,
    headers: &[(String, String)],
) -> Result<reqwest::RequestBuilder> {
    let mut map = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("invalid header name: {k}"))?;
        let value = reqwest::header::HeaderValue::from_str(v)
            .with_context(|| format!("invalid header value for header {k}"))?;
        map.insert(name, value);
    }
    Ok(req.headers(map))
}

#[cfg(test)]
mod tests {
    use super::{apply_headers, ApiEnvelope};

    /// 回归测试：completion 请求同时经过 `.json()` 和显式 `content-type` 时，
    /// 最终请求只能带一个 content-type 头（重复会被服务端判为 HTTP 415）。
    #[test]
    fn apply_headers_replaces_reqwest_json_content_type() {
        let req = reqwest::Client::new()
            .post("http://127.0.0.1:1/api/v0/chat/completion")
            .json(&serde_json::json!({"a": 1}));
        let req = apply_headers(
            req,
            &[
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "*/*".into()),
            ],
        )
        .expect("apply_headers");
        let req = req.build().expect("build request");
        let values = req.headers().get_all(reqwest::header::CONTENT_TYPE);
        assert_eq!(
            values.iter().count(),
            1,
            "content-type must appear exactly once, got {:?}",
            values.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            req.headers()[reqwest::header::CONTENT_TYPE],
            "application/json"
        );
    }

    /// 头列表不含 content-type 时，`.json()` 自动生成的那个保持唯一。
    #[test]
    fn apply_headers_keeps_single_content_type_when_not_listed() {
        let req = reqwest::Client::new()
            .post("http://127.0.0.1:1/api/v0/chat/create_pow_challenge")
            .json(&serde_json::json!({"target_path": "/api/v0/chat/completion"}));
        let req = apply_headers(req, &[("accept".into(), "*/*".into())]).expect("apply_headers");
        let req = req.build().expect("build request");
        assert_eq!(
            req.headers()
                .get_all(reqwest::header::CONTENT_TYPE)
                .iter()
                .count(),
            1
        );
    }

    /// 正常响应：顶层 code=0，data.biz_code=0，取回 biz_data。
    #[test]
    fn into_biz_data_returns_payload_on_success() {
        let env: ApiEnvelope = serde_json::from_str(
            r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"file-1"}}}"#,
        )
        .expect("parse envelope");
        let biz = env.into_biz_data("upload").expect("biz data");
        assert_eq!(biz["id"], "file-1");
    }

    /// 回归测试：业务码放在 `data.biz_code` 时必须被识别。
    /// 线上上传被风控会返回 `data.biz_code=14` / `data.biz_msg="user is muted"`，
    /// 只看顶层会漏判，最终报出难以定位的 `file missing id`。
    #[test]
    fn into_biz_data_surfaces_business_code_inside_data() {
        let env: ApiEnvelope = serde_json::from_str(
            r#"{"code":0,"msg":"","data":{"biz_code":14,"biz_msg":"user is muted","biz_data":{"is_muted":1,"mute_until":1794738505.0}}}"#,
        )
        .expect("parse envelope");
        let err = env.into_biz_data("upload").expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("biz_code=14"), "unexpected error: {msg}");
        assert!(msg.contains("user is muted"), "unexpected error: {msg}");
    }

    /// 顶层业务码（如鉴权失败 40003）仍然生效。
    #[test]
    fn into_biz_data_surfaces_top_level_code() {
        let env: ApiEnvelope =
            serde_json::from_str(r#"{"code":40003,"msg":"Authorization Failed (invalid token)"}"#)
                .expect("parse envelope");
        let err = env.into_biz_data("pow challenge").expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("biz_code=40003"), "unexpected error: {msg}");
        assert!(
            msg.contains("Authorization Failed"),
            "unexpected error: {msg}"
        );
    }
}

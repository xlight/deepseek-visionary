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

/// 鉴权失败（token 失效）的业务码：DeepSeek 用 `40003` +
/// `Authorization Failed (invalid token)` 表达。
pub const AUTH_FAILURE_CODE: i64 = 40003;

/// 账号被风控静音的业务码（上传等写操作被拒）。
pub const MUTED_CODE: i64 = 14;

/// 鉴权失败（token 失效/被服务端轮换）的可识别错误。
///
/// 流水线据此触发一次自动续期并重试（见 `pipeline.rs`）。
#[derive(Debug)]
pub struct AuthFailure(pub String);

impl std::fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AuthFailure {}

/// 瞬时错误（限流 / 服务端错误）——可退避重试。
#[derive(Debug)]
pub struct TransientFailure(pub String);

impl std::fmt::Display for TransientFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TransientFailure {}

/// 判断 HTTP 状态 + 响应体是否表示鉴权失败。
///
/// 只认 401 与明确的鉴权文案：**不把 403 当鉴权失败**——上传接口在 PoW 校验失败时
/// 也返回 403，那是另一类问题（重新签发 PoW 才有意义，刷新 token 没用）。
pub fn looks_like_auth_failure(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("authorization failed")
        || lower.contains("invalid token")
        || lower.contains("\"40003\"")
}

impl ApiEnvelope {
    /// 有效业务码（顶层或 `data` 内，均忽略 0）。
    fn effective_biz_code(&self) -> i64 {
        self.biz_code
            .filter(|c| *c != 0)
            .or_else(|| {
                self.data
                    .as_ref()
                    .and_then(|d| d.biz_code)
                    .filter(|c| *c != 0)
            })
            .or(self.code)
            .filter(|c| *c != 0)
            .unwrap_or(0)
    }

    /// 业务错误描述（无业务错误时返回 `None`）。
    ///
    /// DeepSeek 的业务码有两个位置：顶层 `biz_code`/`code`，以及 `data.biz_code`。
    /// 线上响应实际把业务码放在 `data` 里（顶层 `code` 恒为 0），
    /// 只读顶层会漏判业务错误，最终报出 `file missing id` 这类难以定位的下游错误。
    pub fn biz_error_message(&self, context: &str) -> Option<String> {
        let biz = self.effective_biz_code();
        if biz == 0 {
            return None;
        }
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
        let mut out = format!("{context} failed: biz_code={biz}, msg={msg}");
        if biz == MUTED_CODE {
            if let Some(until) = self
                .data
                .as_ref()
                .and_then(|d| d.biz_data.get("mute_until"))
                .and_then(|v| v.as_f64())
            {
                out.push_str(&format!(
                    "; account uploads are muted until {} (unix {})",
                    format_epoch_utc(until as i64),
                    until as i64
                ));
            }
        }
        Some(out)
    }

    /// 校验业务码为 0，返回 `data.biz_data`。
    pub fn into_biz_data(self, context: &str) -> Result<serde_json::Value> {
        if let Some(msg) = self.biz_error_message(context) {
            return Err(anyhow!("{msg}"));
        }
        self.data
            .map(|d| d.biz_data)
            .context(format!("{context}: missing data.biz_data"))
    }

    /// 是否表示鉴权失败（token 失效）。
    pub fn is_auth_failure(&self) -> bool {
        if self.effective_biz_code() == AUTH_FAILURE_CODE {
            return true;
        }
        let top = self.msg.as_deref().unwrap_or_default().to_ascii_lowercase();
        let inner = self
            .data
            .as_ref()
            .and_then(|d| d.biz_msg.as_deref())
            .unwrap_or_default()
            .to_ascii_lowercase();
        [top, inner]
            .iter()
            .any(|m| m.contains("authorization failed") || m.contains("invalid token"))
    }
}

/// unix 时间戳 → `YYYY-MM-DD HH:MM UTC`（Howard Hinnant 的 civil_from_days 算法）。
///
/// 只用于把风控返回的 `mute_until` 变成人看得懂的绝对时间，避免额外引入时间库依赖。
pub fn format_epoch_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02} UTC")
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
        if looks_like_auth_failure(status, &text) {
            return Err(AuthFailure(format!("GET {path} returned HTTP {status}: {text}")).into());
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(
                TransientFailure(format!("GET {path} returned HTTP {status}: {text}")).into(),
            );
        }
        if status != StatusCode::OK {
            return Err(anyhow!("GET {path} returned HTTP {status}: {text}"));
        }
        let envelope: ApiEnvelope =
            serde_json::from_str(&text).with_context(|| format!("parse GET {path} response"))?;
        if envelope.is_auth_failure() {
            let detail = envelope
                .biz_error_message(path)
                .unwrap_or_else(|| "invalid token".to_string());
            return Err(AuthFailure(detail).into());
        }
        Ok(envelope)
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
                    if looks_like_auth_failure(status, &text) {
                        return Err(AuthFailure(format!(
                            "POST {path} returned HTTP {status}: {text}"
                        ))
                        .into());
                    }
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        return Err(TransientFailure(format!(
                            "POST {path} returned HTTP {status}: {text}"
                        ))
                        .into());
                    }
                    if status != StatusCode::OK {
                        return Err(anyhow!("POST {path} returned HTTP {status}: {text}"));
                    }
                    let envelope: ApiEnvelope = serde_json::from_str(&text)
                        .with_context(|| format!("parse POST {path} response"))?;
                    if envelope.is_auth_failure() {
                        let detail = envelope
                            .biz_error_message(path)
                            .unwrap_or_else(|| "invalid token".to_string());
                        return Err(AuthFailure(detail).into());
                    }
                    return Ok(envelope);
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
        if looks_like_auth_failure(status, &text) {
            return Err(AuthFailure(format!("POST {path} returned HTTP {status}: {text}")).into());
        }
        if status == StatusCode::FORBIDDEN {
            return Err(anyhow!("POST {path} returned 403 after PoW: {text}"));
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(
                TransientFailure(format!("POST {path} returned HTTP {status}: {text}")).into(),
            );
        }
        if status != StatusCode::OK {
            return Err(anyhow!("POST {path} returned HTTP {status}: {text}"));
        }
        let envelope: ApiEnvelope =
            serde_json::from_str(&text).with_context(|| format!("parse POST {path} response"))?;
        if envelope.is_auth_failure() {
            let detail = envelope
                .biz_error_message(path)
                .unwrap_or_else(|| "invalid token".to_string());
            return Err(AuthFailure(detail).into());
        }
        Ok(envelope)
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
    use super::{apply_headers, format_epoch_utc, looks_like_auth_failure, ApiEnvelope};

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

    #[test]
    fn is_auth_failure_detects_biz_code_40003() {
        let env: ApiEnvelope =
            serde_json::from_str(r#"{"code":40003,"msg":"Authorization Failed (invalid token)"}"#)
                .unwrap();
        assert!(env.is_auth_failure());

        // 线上实际把业务码放在 data 里
        let env: ApiEnvelope = serde_json::from_str(
            r#"{"code":0,"data":{"biz_code":40003,"biz_msg":"Authorization Failed (invalid token)"}}"#,
        )
        .unwrap();
        assert!(env.is_auth_failure());
    }

    #[test]
    fn is_auth_failure_detects_message_text() {
        let env: ApiEnvelope = serde_json::from_str(r#"{"code":0,"msg":"invalid token"}"#).unwrap();
        assert!(env.is_auth_failure());
    }

    #[test]
    fn is_auth_failure_false_for_normal_response() {
        let env: ApiEnvelope =
            serde_json::from_str(r#"{"code":0,"data":{"biz_code":0,"biz_data":{"ok":true}}}"#)
                .unwrap();
        assert!(!env.is_auth_failure());
    }

    #[test]
    fn looks_like_auth_failure_ignores_403() {
        // 403 是 PoW 校验失败，不是 token 失效：刷新 token 没用，必须区分开。
        assert!(!looks_like_auth_failure(
            reqwest::StatusCode::FORBIDDEN,
            "forbidden"
        ));
        assert!(looks_like_auth_failure(
            reqwest::StatusCode::UNAUTHORIZED,
            "unauthorized"
        ));
        assert!(looks_like_auth_failure(
            reqwest::StatusCode::OK,
            r#"{"msg":"Authorization Failed (invalid token)"}"#
        ));
    }

    #[test]
    fn muted_error_message_includes_deadline() {
        let env: ApiEnvelope = serde_json::from_str(
            r#"{"code":0,"data":{"biz_code":14,"biz_msg":"user is muted","biz_data":{"is_muted":1,"mute_until":1794738505}}}"#,
        )
        .unwrap();
        let msg = env.biz_error_message("upload").unwrap();
        assert!(msg.contains("biz_code=14"), "{msg}");
        assert!(msg.contains("user is muted"), "{msg}");
        assert!(msg.contains("2026-11-15 10:28 UTC"), "{msg}");
    }

    #[test]
    fn format_epoch_utc_known_values() {
        assert_eq!(format_epoch_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_epoch_utc(1_700_000_000), "2023-11-14 22:13 UTC");
        assert_eq!(format_epoch_utc(1794738505), "2026-11-15 10:28 UTC");
    }
}

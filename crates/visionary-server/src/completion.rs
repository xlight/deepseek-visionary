//! Vision completion：流式请求 + SSE 解析（对照 Python 版 `server.py::_vision_completion`）。
//!
//! 注意 TLS 指纹：Python 版用 curl_cffi `impersonate="chrome131"`。本模块将 HTTP 层
//! 抽象为 `CompletionTransport` trait：
//! - `ReqwestTransport`：普通 reqwest（rustls），spike 1.1 验证是否被 403
//! - （待定）`ImpersonateTransport`：`impersonate` crate 模拟 Chrome131 指纹，spike 1.2 结论后落地
//!
//! SSE 解析逻辑与 Python 逐行对齐：`data:` 前缀行、`[DONE]` 终止、
//! `type=error` 抛错、`response_message_id` 提取、`v` 字符串增量拼接。

use crate::client::ApiClient;
use crate::hif::HifAuth;
use crate::pow;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::StatusCode;
use std::time::Duration;

/// completion 请求的 body（对应 Python `_vision_completion` 的 `body`）。
#[derive(Debug, serde::Serialize)]
pub struct CompletionBody {
    pub chat_session_id: String,
    pub parent_message_id: Option<String>,
    pub model_type: String,
    pub prompt: String,
    pub ref_file_ids: Vec<String>,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<serde_json::Value>,
    pub preempt: bool,
}

/// completion 的 HTTP 传输层抽象（TLS 指纹可切换点）。
trait CompletionTransport {
    /// 发起流式 POST，返回响应体字节流。
    fn post_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
        cookies: &[(String, String)],
        body: &CompletionBody,
    ) -> impl std::future::Future<
        Output = Result<impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>>>,
    >;
}

/// 普通 reqwest 实现（rustls）。
struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("build completion http client")?;
        Ok(Self { client })
    }
}

impl CompletionTransport for ReqwestTransport {
    async fn post_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
        cookies: &[(String, String)],
        body: &CompletionBody,
    ) -> Result<impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>>> {
        let mut req = self.client.post(url).json(body);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        for (k, v) in cookies {
            req = req.header(reqwest::header::COOKIE, format!("{k}={v}"));
        }
        let resp = req.send().await.context("completion POST failed")?;
        let status = resp.status();
        if status != StatusCode::OK {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Vision API returned HTTP {status}: {}",
                &text[..text.len().min(300)]
            ));
        }
        Ok(resp.bytes_stream())
    }
}

/// 执行 vision completion，返回 (回答文本, 新的 parent_message_id)。
///
/// 对应 Python `_vision_completion`。`on_token` 为可选流式回调：
/// 解析到内容增量时先回调再收集（CLI 流式打印用）；`None` 时仅收集（MCP / `--json`）。
///
/// 约束：`F: Send`——rmcp `#[tool]` 宏要求 handler future Send，`&mut dyn FnMut` 非 Send，
/// 故用泛型（design 决策 2 / Risks）。
///
/// 8 参数为共享流水线签名（design 决策 2 明确透传回调），保持单函数复用。
#[allow(clippy::too_many_arguments)]
pub async fn vision_completion<F>(
    client: &ApiClient,
    hif: &HifAuth,
    session_id: &str,
    vision_file_id: &str,
    prompt: &str,
    thinking: bool,
    parent_message_id: Option<&str>,
    on_token: Option<F>,
) -> Result<(String, Option<String>)>
where
    F: FnMut(&str) + Send,
{
    let config = &client.config;
    let token = config.credentials().user_token;

    // completion 前 PoW（对应 `_run_vision_pipeline` 中的第三步）
    let envelope = client
        .post_json(
            "/api/v0/chat/create_pow_challenge",
            &serde_json::json!({ "target_path": "/api/v0/chat/completion" }),
            Duration::from_secs(30),
        )
        .await?;
    let biz = envelope.into_biz_data("pow challenge")?;
    let challenge: pow::Challenge = serde_json::from_value(
        biz.get("challenge")
            .cloned()
            .context("pow challenge response missing `challenge`")?,
    )
    .context("parse pow challenge")?;
    let pow_header = pow::PoWSolver::solve_challenge(&challenge)?;

    let hif_headers = hif.get_headers().await?;

    let mut headers: Vec<(String, String)> = vec![
        ("accept".into(), "*/*".into()),
        (
            "accept-language".into(),
            format!(
                "{},{loc};q=0.9,en;q=0.8",
                config.client_locale,
                loc = config.client_locale
            ),
        ),
        ("authorization".into(), format!("Bearer {token}")),
        ("content-type".into(), "application/json".into()),
        ("priority".into(), "u=1, i".into()),
        ("origin".into(), config.base_url.clone()),
        (
            "referer".into(),
            format!("{}/a/chat/s/{}", config.base_url, session_id),
        ),
        ("x-app-version".into(), config.app_version.clone()),
        ("x-client-locale".into(), config.client_locale.clone()),
        ("x-client-platform".into(), "web".into()),
        ("x-client-timezone-offset".into(), "28800".into()),
        ("x-client-version".into(), config.app_version.clone()),
        ("x-ds-pow-response".into(), pow_header),
    ];
    headers.extend(hif_headers);

    let body = CompletionBody {
        chat_session_id: session_id.to_string(),
        parent_message_id: parent_message_id.map(String::from),
        model_type: "vision".into(),
        prompt: prompt.to_string(),
        ref_file_ids: vec![vision_file_id.to_string()],
        thinking_enabled: thinking,
        search_enabled: false,
        action: None,
        preempt: false,
    };

    // TLS 指纹切换点：spike 1.1/1.2 结论后在此替换 transport。
    // 先以普通 reqwest 跑通（若 completion 被 403，再切 impersonate）。
    let transport = ReqwestTransport::new()?;
    let url = format!("{}/api/v0/chat/completion", config.base_url);
    let cookies = config.cookies();
    let mut stream = transport
        .post_stream(&url, &headers, &cookies, &body)
        .await?;

    parse_sse(&mut stream, on_token).await
}

/// 逐行解析 SSE 流（对应 Python `_vision_completion` 的循环体）。
///
/// `on_token` 为可选流式回调：解析到 `v` 字符串或 `type=text` 增量时
/// 先回调再收集，收集逻辑天然复用，无需双分支。
async fn parse_sse<S, F>(
    stream: &mut S,
    mut on_token: Option<F>,
) -> Result<(String, Option<String>)>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    F: FnMut(&str) + Send,
{
    let mut text_parts: Vec<String> = Vec::new();
    let mut new_parent_message_id: Option<String> = None;
    let mut line_buf: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("completion stream error")?;
        for byte in chunk {
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                line_buf.clear();
                if !line.starts_with("data:") {
                    continue;
                }
                let payload = line[5..].trim().to_string();
                if payload == "[DONE]" {
                    return Ok((text_parts.concat(), new_parent_message_id));
                }
                let event: serde_json::Value = match serde_json::from_str(&payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if event.get("type").and_then(|t| t.as_str()) == Some("error") {
                    return Err(anyhow!(
                        "Vision error: {}",
                        event
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("unknown error")
                    ));
                }

                // 提取 message_id（对应 Python 的多级查找）
                let msg_id = event
                    .get("response_message_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        event
                            .get("v")
                            .and_then(|v| v.get("response"))
                            .and_then(|r| r.get("message_id"))
                            .and_then(|v| v.as_str())
                    })
                    .or_else(|| {
                        event
                            .get("message_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| event.get("msg_id").and_then(|v| v.as_str()))
                    });
                if let Some(id) = msg_id {
                    new_parent_message_id = Some(id.to_string());
                }

                // 文本增量：`v` 为字符串时直接拼接
                if let Some(v) = event.get("v").and_then(|v| v.as_str()) {
                    if let Some(cb) = on_token.as_mut() {
                        cb(v);
                    }
                    text_parts.push(v.to_string());
                }

                // 备选文本格式：type == "text"
                if event.get("type").and_then(|t| t.as_str()) == Some("text") {
                    let txt = event
                        .get("text")
                        .or_else(|| event.get("content"))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let joined = match txt {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Array(arr) => arr
                            .iter()
                            .map(|x| match x {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Object(o) => o
                                    .get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                other => other.to_string(),
                            })
                            .collect::<String>(),
                        _ => String::new(),
                    };
                    if !joined.is_empty() {
                        if let Some(cb) = on_token.as_mut() {
                            cb(&joined);
                        }
                        text_parts.push(joined);
                    }
                }
            } else {
                line_buf.push(byte);
            }
        }
    }

    Ok((text_parts.concat(), new_parent_message_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模拟 SSE 流验证解析逻辑。
    #[tokio::test]
    async fn parse_sse_extracts_text_and_message_id() {
        let sse = concat!(
            "data: {\"v\":\"你好\",\"response_message_id\":\"m1\"}\n\n",
            "data: {\"v\":\"，世界\"}\n\n",
            "data: {\"type\":\"text\",\"text\":\"!\"}\n\n",
            "data: [DONE]\n\n"
        );
        let stream = futures_util::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(
            sse.as_bytes().to_vec(),
        ))]);
        let mut boxed: Box<
            dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
        > = Box::new(stream);
        let (text, parent_id) = parse_sse(&mut boxed, None::<fn(&str)>).await.unwrap();
        assert_eq!(text, "你好，世界!");
        assert_eq!(parent_id.as_deref(), Some("m1"));
    }

    #[tokio::test]
    async fn parse_sse_errors_on_error_event() {
        let sse = "data: {\"type\":\"error\",\"content\":\"content filter\"}\n\n";
        let stream = futures_util::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(
            sse.as_bytes().to_vec(),
        ))]);
        let mut boxed: Box<
            dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
        > = Box::new(stream);
        let err = parse_sse(&mut boxed, None::<fn(&str)>).await.unwrap_err();
        assert!(err.to_string().contains("content filter"));
    }

    #[tokio::test]
    async fn parse_sse_streams_tokens_via_callback() {
        // 流式分支：回调逐块触发且顺序一致，收集结果与无回调时一致。
        let sse = concat!(
            "data: {\"v\":\"你\",\"response_message_id\":\"m1\"}\n\n",
            "data: {\"v\":\"好\"}\n\n",
            "data: {\"type\":\"text\",\"text\":\"！\"}\n\n",
            "data: [DONE]\n\n"
        );
        let stream = futures_util::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(
            sse.as_bytes().to_vec(),
        ))]);
        let mut boxed: Box<
            dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
        > = Box::new(stream);

        let mut streamed = String::new();
        let (text, parent_id) = parse_sse(&mut boxed, Some(|tok: &str| streamed.push_str(tok)))
            .await
            .unwrap();
        assert_eq!(
            streamed, "你好！",
            "callback should receive all deltas in order"
        );
        assert_eq!(
            text, "你好！",
            "collected result should equal streamed content"
        );
        assert_eq!(parent_id.as_deref(), Some("m1"));
    }
}

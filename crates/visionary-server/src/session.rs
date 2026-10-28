//! 会话创建与续聊状态持久化（对照 Python 版 `server.py`）。
//!
//! - `create_session`：调用 `/api/v0/chat_session/create`（agent=chat）
//!   响应 `data.biz_data = { "chat_session": { "id": ... }, "ttl_seconds": ... }`
//!   （旧版为平铺的 `biz_data.id`，两种形态都兼容——见 `extract_session_id`）
//! - session.json：`~/.deepseek-visionary/session.json`，记录 `{ session_id, parent_message_id }`，
//!   供 `continue_conversation=true` / 显式 `session_id` 时复用。

use crate::client::ApiClient;
use crate::config;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

/// 会话状态（对应 Python 存到 session.json 的 dict）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, rename = "parent_message_id")]
    pub parent_message_id: Option<String>,
}

/// 内存态 + 磁盘态的会话状态（对应 Python 全局 `_last_session_state` + session.json）。
pub struct SessionStore {
    state: Mutex<Option<SessionState>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    /// 加载上次持久化的会话状态（内存优先，对应 Python `_load_session_state`）。
    pub fn load(&self) -> Option<SessionState> {
        let guard = self.state.lock().ok()?;
        if let Some(s) = guard.as_ref() {
            return Some(s.clone());
        }
        let file = config::session_file().ok()?;
        let raw = std::fs::read_to_string(&file).ok()?;
        let state: SessionState = serde_json::from_str(&raw).ok()?;
        Some(state)
    }

    /// 持久化会话状态（对应 Python `_save_session_state`）。
    pub fn save(&self, state: &SessionState) {
        if let Ok(mut guard) = self.state.lock() {
            *guard = Some(state.clone());
        }
        let Ok(file) = config::session_file() else {
            return;
        };
        let _ = config::write_private_json(&file, state);
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建新的聊天会话，返回 session_id（对应 Python `_create_session`）。
pub async fn create_session(client: &ApiClient) -> Result<String> {
    let envelope = client
        .post_json(
            "/api/v0/chat_session/create",
            &serde_json::json!({ "agent": "chat" }),
            Duration::from_secs(30),
        )
        .await?;
    let biz = envelope.into_biz_data("session create")?;
    extract_session_id(&biz)
        .with_context(|| anyhow!("session create response missing id: {}", preview(&biz)))
}

/// 从 session create 的 `biz_data` 中提取 session_id。
///
/// DeepSeek 网页端当前返回嵌套结构 `biz_data.chat_session.id`；早期版本把会话对象
/// 平铺在 `biz_data` 下（`biz_data.id`）。两种形态都兼容，避免服务端契约漂移直接
/// 打断流水线。id 允许是字符串或整数（与 SSE 的 `response_message_id` 一致处理）。
fn extract_session_id(biz: &serde_json::Value) -> Option<String> {
    biz.get("chat_session")
        .and_then(|s| s.get("id"))
        .or_else(|| biz.get("id"))
        .and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|i| i.to_string()))
        })
}

/// 错误信息中的响应预览（按字符截断，避免日志过长或切断多字节字符）。
fn preview(biz: &serde_json::Value) -> String {
    let s = biz.to_string();
    let head: String = s.chars().take(200).collect();
    if s.chars().count() > 200 {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 服务端当前契约：session 嵌在 `biz_data.chat_session` 下。
    #[test]
    fn session_id_from_nested_chat_session() {
        let biz = serde_json::json!({
            "chat_session": {
                "id": "e6131464-81a8-4d51-a38f-1bf913537002",
                "title": "",
                "updated_at": 1_760_000_000,
                "model_type": "vision"
            },
            "ttl_seconds": 3600
        });
        assert_eq!(
            extract_session_id(&biz).as_deref(),
            Some("e6131464-81a8-4d51-a38f-1bf913537002")
        );
    }

    /// 旧契约：session 平铺在 `biz_data` 下，id 直接在顶层。
    #[test]
    fn session_id_from_legacy_flat_shape() {
        let biz = serde_json::json!({ "id": "legacy-session-id" });
        assert_eq!(
            extract_session_id(&biz).as_deref(),
            Some("legacy-session-id")
        );
    }

    /// id 为整数时也接受（与服务端 SSE 的 `response_message_id` 一致）。
    #[test]
    fn session_id_accepts_numeric_id() {
        let biz = serde_json::json!({ "chat_session": { "id": 42 } });
        assert_eq!(extract_session_id(&biz).as_deref(), Some("42"));
    }

    /// 两种形态都取不到 id 时返回 None（由调用方转成明确错误）。
    #[test]
    fn session_id_missing_returns_none() {
        assert!(
            extract_session_id(&serde_json::json!({ "chat_session": { "title": "x" } })).is_none()
        );
        assert!(extract_session_id(&serde_json::json!({})).is_none());
        assert!(extract_session_id(&serde_json::Value::Null).is_none());
    }

    /// 预览截断不切断多字节字符。
    #[test]
    fn preview_truncates_on_char_boundary() {
        let biz = serde_json::json!({ "title": "图".repeat(500) });
        let p = preview(&biz);
        assert!(p.ends_with('\u{2026}'), "unexpected: {p}");
        assert!(p.chars().count() <= 201, "unexpected: {p}");
    }
}

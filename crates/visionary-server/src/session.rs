//! 会话创建与续聊状态持久化（对照 Python 版 `server.py`）。
//!
//! - `create_session`：调用 `/api/v0/chat_session/create`（agent=chat）
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
    biz.get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .with_context(|| anyhow!("session create response missing id"))
}

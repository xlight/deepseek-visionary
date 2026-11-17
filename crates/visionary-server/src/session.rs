//! 会话创建与续聊状态持久化（对照 Python 版 `server.py`）。
//!
//! - `create_session`：调用 `/api/v0/chat_session/create`（agent=chat）
//!   响应 `data.biz_data = { "chat_session": { "id": ... }, "ttl_seconds": ... }`
//!   （旧版为平铺的 `biz_data.id`，两种形态都兼容——见 `extract_session_id`）
//! - session.json：`~/.deepseek-visionary/session.json`，按账号 id 分桶记录
//!   `{ session_id, parent_message_id }`，供 `continue_conversation=true` /
//!   显式 `session_id` 时复用。

use crate::client::ApiClient;
use crate::config;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
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

/// 单账号模式使用的默认会话桶。
pub const DEFAULT_BUCKET: &str = "default";

/// session.json 的新格式：按账号 id 保存会话状态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionFile {
    sessions: HashMap<String, SessionState>,
}

/// 内存态 + 磁盘态的会话状态（对应 Python 全局 `_last_session_state` + session.json）。
///
/// 会话与账号绑定，因此内存和磁盘均按账号 id 分桶，避免轮询换号后误续上
/// 另一个账号的会话。
pub struct SessionStore {
    state: Mutex<HashMap<String, SessionState>>,
    /// 旧版平铺 session.json 归位到哪个账号桶。
    legacy_owner: String,
    /// 会话文件路径；构造时解析一次，便于测试注入临时目录。
    file: Option<PathBuf>,
}

impl SessionStore {
    /// 单账号模式：旧格式会话归入 `DEFAULT_BUCKET`。
    pub fn new() -> Self {
        Self::with_legacy_owner(None)
    }

    /// 池模式：旧格式会话归入指定账号的桶。
    ///
    /// `None` 与 `new()` 等价；传入真实账号 id 时，迁移不会使用
    /// `DEFAULT_BUCKET` 占位键。
    pub fn with_legacy_owner(account_id: Option<String>) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            legacy_owner: account_id.unwrap_or_else(|| DEFAULT_BUCKET.to_string()),
            file: config::session_file().ok(),
        }
    }

    /// 加载指定账号上次持久化的会话状态（内存优先）。
    ///
    /// 内存未命中时读取 session.json 对应桶；旧版平铺格式会归位到
    /// `legacy_owner` 并立即写回新格式。
    pub fn load(&self, account_id: &str) -> Option<SessionState> {
        let mut guard = self.state.lock().ok()?;
        if let Some(state) = guard.get(account_id) {
            return Some(state.clone());
        }

        let file = self.file.as_ref()?;
        let buckets = self.read_buckets(file);

        // 将磁盘态同步到内存；后续读取无需再次访问文件。
        for (bucket, state) in &buckets {
            guard.entry(bucket.clone()).or_insert_with(|| state.clone());
        }
        buckets.get(account_id).cloned()
    }

    /// 持久化指定账号的会话状态，同时更新内存桶与磁盘文件。
    pub fn save(&self, account_id: &str, state: &SessionState) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        guard.insert(account_id.to_string(), state.clone());

        let Some(file) = self.file.as_ref() else {
            return;
        };
        let mut buckets = self.read_buckets(file);
        for (bucket, bucket_state) in guard.iter() {
            buckets.insert(bucket.clone(), bucket_state.clone());
        }
        if let Err(e) = write_buckets(file, &buckets) {
            tracing::warn!("write session buckets failed: {e:#}");
        }
    }

    /// 读取文件中的全部会话桶；遇到旧版平铺格式时完成一次归位迁移。
    fn read_buckets(&self, file: &std::path::Path) -> HashMap<String, SessionState> {
        let Ok(raw) = std::fs::read_to_string(file) else {
            return HashMap::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return HashMap::new();
        };

        if let Some(sessions) = value.get("sessions") {
            return serde_json::from_value::<HashMap<String, SessionState>>(sessions.clone())
                .unwrap_or_default();
        }

        // 旧格式：顶层直接记录 session_id / parent_message_id。
        if value.get("session_id").is_none() {
            return HashMap::new();
        }
        let Ok(state) = serde_json::from_value::<SessionState>(value) else {
            return HashMap::new();
        };

        let mut buckets = HashMap::new();
        buckets.insert(self.legacy_owner.clone(), state);
        if let Err(e) = write_buckets(file, &buckets) {
            tracing::warn!("migrate legacy session file failed: {e:#}");
        }
        buckets
    }

    /// 仅供测试注入临时 session.json 路径。
    #[cfg(test)]
    fn with_test_file(file: PathBuf, legacy_owner: Option<String>) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            legacy_owner: legacy_owner.unwrap_or_else(|| DEFAULT_BUCKET.to_string()),
            file: Some(file),
        }
    }
}

/// 按新格式原子写入会话文件。
fn write_buckets(file: &std::path::Path, buckets: &HashMap<String, SessionState>) -> Result<()> {
    config::write_private_json(
        file,
        &SessionFile {
            sessions: buckets.clone(),
        },
    )
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

    /// 创建测试专用 session.json 路径，所有会话测试均不得触碰真实用户目录。
    fn temp_session_file(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "visionary-session-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp session dir");
        dir.join("session.json")
    }

    fn state(session_id: &str, parent_message_id: &str) -> SessionState {
        SessionState {
            session_id: Some(session_id.to_string()),
            parent_message_id: Some(parent_message_id.to_string()),
        }
    }

    /// 不同账号的内存态互相隔离。
    #[test]
    fn buckets_keep_memory_state_separate() {
        let file = temp_session_file("memory");
        let store = SessionStore::with_test_file(file.clone(), None);

        store.save("account-a", &state("session-a", "parent-a"));
        store.save("account-b", &state("session-b", "parent-b"));

        assert_eq!(
            store.load("account-a").and_then(|s| s.session_id),
            Some("session-a".to_string())
        );
        assert_eq!(
            store.load("account-b").and_then(|s| s.session_id),
            Some("session-b".to_string())
        );
        assert!(store.load("account-c").is_none());

        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// 磁盘态按账号分桶保存，重新创建 store 后仍可分别读取。
    #[test]
    fn buckets_persist_to_file() {
        let file = temp_session_file("file");
        let store = SessionStore::with_test_file(file.clone(), None);

        store.save("account-a", &state("session-a", "parent-a"));
        store.save("account-b", &state("session-b", "parent-b"));

        let raw = std::fs::read_to_string(&file).expect("read session file");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parse session file");
        assert_eq!(value["sessions"]["account-a"]["session_id"], "session-a");
        assert_eq!(value["sessions"]["account-b"]["session_id"], "session-b");
        assert!(value.get("session_id").is_none());

        let reopened = SessionStore::with_test_file(file.clone(), None);
        assert_eq!(
            reopened.load("account-a").and_then(|s| s.session_id),
            Some("session-a".to_string())
        );
        assert_eq!(
            reopened.load("account-b").and_then(|s| s.session_id),
            Some("session-b".to_string())
        );
        assert!(reopened.load("account-c").is_none());

        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// 旧版平铺格式归入指定真实账号桶，并立即升级写回新格式。
    #[test]
    fn legacy_flat_format_migrates_to_owner_bucket_once() {
        let file = temp_session_file("legacy");
        std::fs::write(
            &file,
            r#"{"session_id":"legacy-session","parent_message_id":"legacy-parent"}"#,
        )
        .expect("write legacy session file");

        let store = SessionStore::with_test_file(file.clone(), Some("account-a".to_string()));
        assert_eq!(
            store.load("account-a").and_then(|s| s.session_id),
            Some("legacy-session".to_string())
        );
        assert!(store.load("account-b").is_none());

        let raw = std::fs::read_to_string(&file).expect("read migrated session file");
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("parse migrated session file");
        assert_eq!(
            value["sessions"]["account-a"]["session_id"],
            "legacy-session"
        );
        assert!(value.get("session_id").is_none());
        assert!(value["sessions"].get(DEFAULT_BUCKET).is_none());

        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// 单账号模式未提供真实 owner 时，旧格式归入默认桶并写回新格式。
    #[test]
    fn legacy_flat_format_defaults_to_default_bucket() {
        let file = temp_session_file("legacy-default");
        std::fs::write(
            &file,
            r#"{"session_id":"legacy-session","parent_message_id":null}"#,
        )
        .expect("write legacy session file");

        let store = SessionStore::with_test_file(file.clone(), None);
        assert_eq!(
            store.load(DEFAULT_BUCKET).and_then(|s| s.session_id),
            Some("legacy-session".to_string())
        );

        let raw = std::fs::read_to_string(&file).expect("read migrated session file");
        let value: serde_json::Value =
            serde_json::from_str(&raw).expect("parse migrated session file");
        assert_eq!(
            value["sessions"][DEFAULT_BUCKET]["session_id"],
            "legacy-session"
        );
        assert!(value.get("session_id").is_none());

        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// 换账号后不会串用上一账号的会话。
    #[test]
    fn switching_account_does_not_reuse_session() {
        let file = temp_session_file("switch");
        let store = SessionStore::with_test_file(file.clone(), None);

        store.save("account-a", &state("session-a", "parent-a"));
        assert!(store.load("account-b").is_none());

        store.save("account-b", &state("session-b", "parent-b"));
        assert_eq!(
            store.load("account-a").and_then(|s| s.session_id),
            Some("session-a".to_string())
        );
        assert_eq!(
            store.load("account-b").and_then(|s| s.session_id),
            Some("session-b".to_string())
        );

        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// 显式 session_id 的记录仍严格从所属账号桶读取（pipeline 层负责显式 id 约束）。
    #[test]
    fn explicit_session_id_is_loaded_from_its_bucket() {
        let file = temp_session_file("explicit");
        let store = SessionStore::with_test_file(file.clone(), None);

        store.save("account-a", &state("explicit-session", "parent-a"));
        assert_eq!(
            store.load("account-a").and_then(|s| s.session_id),
            Some("explicit-session".to_string())
        );
        assert!(store.load("account-b").is_none());
    }
}

//! 账号健康只读探针（`GET /api/v0/users/current`）。
//!
//! 用途：查询当前凭据对应的账号是否被上传风控静音（`chat.is_muted`）、
//! 静音截止时间（`chat.mute_until`）、账号稳定标识（`id`，跨 token 轮换不变）
//! 与展示标签。
//!
//! 该探针是**纯只读**调用：不发起上传、不求解 PoW、不消耗配额，
//! 因此可以作为「token 是否有效」的唯一判定入口（取代旧的
//! `POST /api/v0/chat/create_pow_challenge` 校验路径）。
//!
//! 模块名刻意用 `account_health.rs`（单数），与账号池模块 `accounts.rs`（复数）区分。

use crate::client::{format_epoch_utc, ApiClient};
use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// 探针端点。
pub const PROBE_PATH: &str = "/api/v0/users/current";

/// 探针超时上限（秒）。实际超时还要受该模式总预算约束，见 `probe_timeout`。
pub const DEFAULT_PROBE_TIMEOUT_SECS: u64 = 15;

/// 账号健康判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountHealth {
    /// 账号是否被静音（上传风控）。
    pub muted: bool,
    /// 静音截止时间（unix 秒）；缺失表示「未知截止时间」。
    pub mute_until: Option<u64>,
    /// 账号稳定标识（uuid，跨 token 轮换不变）。
    pub account_id: String,
    /// 展示标签（email → mobile_number → id_profile.name → account_id）。
    pub label: String,
}

impl AccountHealth {
    /// 静音截止时间的绝对 UTC 文本（缺失时为 `None`）。
    pub fn mute_until_utc(&self) -> Option<String> {
        self.mute_until.map(|s| format_epoch_utc(s as i64))
    }
}

/// 探针请求的账号档案（`data.biz_data`）。
///
/// 全部字段可选：接口契约变更时按「字段缺失 → 降级」处理，不阻断流程。
#[derive(Debug, Default, serde::Deserialize)]
pub struct AccountProfile {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub mobile_number: Option<String>,
    #[serde(default)]
    pub id_profile: Option<IdProfile>,
    #[serde(default)]
    pub chat: Option<ChatState>,
}

/// 账号资料（展示名）。
#[derive(Debug, Default, serde::Deserialize)]
pub struct IdProfile {
    #[serde(default)]
    pub name: Option<String>,
}

/// 账号的聊天（上传）风控状态。
#[derive(Debug, Default, serde::Deserialize)]
pub struct ChatState {
    /// 静音标记：线上为数字 `0`/`1`，但按布尔解释更稳妥。
    #[serde(default)]
    pub is_muted: Option<serde_json::Value>,
    /// 静音截止时间（unix 秒，线上为浮点）。
    #[serde(default)]
    pub mute_until: Option<f64>,
    /// 服务端给出的静音原因文案（当前为 null，将来可能填充）。
    #[serde(default)]
    pub msg: Option<String>,
}
/// 解析探针返回的账号档案（纯函数，便于单测）。
///
/// 字段缺失时降级：`chat` 缺失 → 按「未静音」处理并记 warning；
/// 静音截止时间缺失 → 「未知截止时间」，不影响静音判定本身。
pub fn parse_health(biz_data: &serde_json::Value) -> AccountHealth {
    let profile: AccountProfile = match serde_json::from_value(biz_data.clone()) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("account health probe: cannot parse profile ({e}); treating as healthy");
            return AccountHealth {
                muted: false,
                mute_until: None,
                account_id: String::new(),
                label: String::new(),
            };
        }
    };

    let account_id = profile.id.clone().unwrap_or_default().trim().to_string();

    let label = profile
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            profile
                .mobile_number
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            profile
                .id_profile
                .as_ref()
                .and_then(|p| p.name.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string)
        .unwrap_or_else(|| account_id.clone());

    let (mut muted, mut mute_until) = (false, None);
    match profile.chat.as_ref() {
        Some(chat) => {
            muted = truthy(chat.is_muted.as_ref());
            mute_until = chat.mute_until.filter(|v| *v > 0.0).map(|v| v as u64);
            if muted {
                if let Some(reason) = chat.msg.as_deref().filter(|m| !m.trim().is_empty()) {
                    tracing::info!("account muted by the server: {reason}");
                }
            }
        }
        None => {
            tracing::warn!(
                "account health probe: response has no 'chat' field; treating account as not muted"
            );
        }
    }

    AccountHealth {
        muted,
        mute_until,
        account_id,
        label,
    }
}

/// 把 JSON 标量按「真值」解释（数字非 0 / 布尔 true / 字符串非空非 0）。
fn truthy(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false")
        }
        Some(_) => false,
    }
}

/// 探针超时：SHALL 短于该模式的总预算。
///
/// 取「模式预算的 1/3」与「15 秒上限」中的较小者——交互登录预算 600s、
/// 自动续期预算 90s，两者不同，写死一个基准会让探针在自动续期时占掉大半预算。
pub fn probe_timeout(budget: Duration) -> Duration {
    let cap = Duration::from_secs(DEFAULT_PROBE_TIMEOUT_SECS);
    let third = budget / 3;
    if third.is_zero() {
        Duration::from_secs(1)
    } else if third < cap {
        third
    } else {
        cap
    }
}

/// 用**候选凭据**探测账号健康。
///
/// 纯只读：一次 GET，无上传、无 PoW、无配额消耗。
pub async fn probe_account(client: &ApiClient) -> Result<AccountHealth> {
    let envelope = client.get_json(PROBE_PATH, &[]).await?;
    let biz = envelope.into_biz_data("account health probe")?;
    Ok(parse_health(&biz))
}

/// 带超时的账号健康探测（外层 `tokio::time::timeout` 包装）。
///
/// 超时返回**可区分**的错误（消息含 "timed out"），由调用方降级为「未知健康」。
pub async fn probe_account_with_timeout(
    client: &ApiClient,
    timeout: Duration,
) -> Result<AccountHealth> {
    match tokio::time::timeout(timeout, probe_account(client)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "account health probe timed out after {}s",
            timeout.as_secs()
        )),
    }
}

/// 探测**指定账号**的健康（只读 GET /api/v0/users/current）。
///
/// 池模式下 `status` / MCP 状态工具要报告「当前使用账号」的凭据状态，
/// 不能拿兼容配置（config.json / 已被忽略的环境变量）去探。
pub async fn probe_account_id(
    config: &Config,
    account_id: &str,
    timeout: Duration,
) -> Result<AccountHealth> {
    let cfg = config.with_credentials(config.credentials_for(account_id));
    let client = ApiClient::new(cfg).context("init api client for account health probe")?;
    probe_account_with_timeout(&client, timeout).await
}

/// 批量刷新的整体预算（秒）：`DEEPSEEK_ACCOUNTS_TIMEOUT`，默认 30。
#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
pub const DEFAULT_ACCOUNTS_TIMEOUT_SECS: u64 = 30;

/// 批量刷新的整体预算（`DEEPSEEK_ACCOUNTS_TIMEOUT`，默认 30s）。
#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
///
/// 单次探针超时另受 `probe_timeout` 约束（≤15s）；N 个探针并发执行，
/// 整体耗时 ≈ 单次探针。
pub fn accounts_budget() -> Duration {
    let secs = std::env::var("DEEPSEEK_ACCOUNTS_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_ACCOUNTS_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// 账号池批量刷新时，单个账号的探针结论。
#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 健康（未静音）。
    Healthy,
    /// 被静音（上传风控）。
    Muted { until: Option<u64> },
    /// 凭据失效（探针返回鉴权失败 40003）。
    Expired,
    /// 探针失败（网络 / 超时 / 其它）——**不代表**账号不可用。
    Failed(String),
}

#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
impl ProbeOutcome {
    /// 该账号当前是否可用（静音账号在到期前不可用，但状态仍需记录）。
    pub fn is_usable(&self) -> bool {
        matches!(self, ProbeOutcome::Healthy | ProbeOutcome::Muted { .. })
    }

    /// 是否应把账号标记为不可用（凭据失效）。
    pub fn marks_unusable(&self) -> bool {
        matches!(self, ProbeOutcome::Expired)
    }

    /// 人类可读的一行摘要。
    pub fn summary(&self) -> String {
        match self {
            ProbeOutcome::Healthy => "healthy".to_string(),
            ProbeOutcome::Muted { until: Some(u) } => {
                format!("muted until {}", format_epoch_utc(*u as i64))
            }
            ProbeOutcome::Muted { until: None } => "muted (unknown deadline)".to_string(),
            ProbeOutcome::Expired => "expired (credentials rejected)".to_string(),
            ProbeOutcome::Failed(e) => format!("probe failed: {e}"),
        }
    }
}

/// 批量刷新账号池健康状态：对池中每个账号各发一次**只读**探针，**并发**执行。
///
/// N 个只读探针无副作用（无上传、无 PoW、无浏览器启动），因此整体耗时 ≈ 单次探针，
/// 而不是 N 倍。单个账号探针失败不影响其它账号。
///
/// `budget` 是整体预算（默认 30s，见 `DEEPSEEK_ACCOUNTS_TIMEOUT`）；单次探针超时
/// 取 `probe_timeout(budget)`（≤15s），超时的探针按「探针失败」处理。
#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
pub async fn refresh_pool(config: &Config, budget: Duration) -> Vec<(String, ProbeOutcome)> {
    let pool = config.pool();
    let accounts: Vec<crate::accounts::PoolAccount> = pool.accounts().to_vec();
    if accounts.is_empty() {
        return Vec::new();
    }
    let per_probe = probe_timeout(budget);
    let futures = accounts.iter().map(|acct| {
        // 每个账号用**它自己的**凭据探针（候选配置不污染当前生效凭据）。
        let cfg = config.with_credentials(acct.credentials());
        let id = acct.account_id.clone();
        async move {
            let outcome = match probe_config(&cfg, per_probe).await {
                Ok(health) => {
                    if health.muted {
                        ProbeOutcome::Muted {
                            until: health.mute_until,
                        }
                    } else {
                        ProbeOutcome::Healthy
                    }
                }
                Err(e) => {
                    if e.downcast_ref::<crate::client::AuthFailure>().is_some() {
                        // 凭据被服务端拒绝：账号级不可用。
                        ProbeOutcome::Expired
                    } else {
                        // 网络 / 超时 / 其它：链路问题，SHALL NOT 标记账号不可用。
                        ProbeOutcome::Failed(format!("{e:#}"))
                    }
                }
            };
            (id, outcome)
        }
    });
    futures_util::future::join_all(futures).await
}

/// 把批量探针结论应用到账号池（内存态）。
///
/// 返回是否有状态变化（调用方据此决定是否落盘）。
/// - 静音 → 记 muted + mute_until
/// - 凭据失效 → 记 expired
/// - 探针失败 → **不改状态**（链路问题不等于账号坏）
/// - 健康 → 清掉过期的静音/失效标记（静音到期后惰性恢复）
#[allow(dead_code)] // 由账号管理 CLI（accounts status）调用
pub fn apply_probe_outcomes(
    pool: &mut crate::accounts::AccountPool,
    outcomes: &[(String, ProbeOutcome)],
) -> bool {
    let mut changed = false;
    for (id, outcome) in outcomes {
        let Some(account) = pool.find_mut(id) else {
            continue;
        };
        match outcome {
            ProbeOutcome::Healthy => {
                if account.status != crate::accounts::AccountStatus::Healthy {
                    account.status = crate::accounts::AccountStatus::Healthy;
                    account.mute_until = None;
                    changed = true;
                }
            }
            ProbeOutcome::Muted { until } => {
                let target = crate::accounts::AccountStatus::Muted;
                if account.status != target || account.mute_until != *until {
                    account.status = target;
                    account.mute_until = *until;
                    changed = true;
                }
            }
            ProbeOutcome::Expired => {
                let target = crate::accounts::AccountStatus::Expired;
                if account.status != target {
                    account.status = target;
                    account.mute_until = None;
                    changed = true;
                }
            }
            ProbeOutcome::Failed(_) => {}
        }
    }
    changed
}

/// 用**配置中当前生效的凭据**探测账号健康。
///
/// 供 `status` / `doctor` / MCP 状态工具使用——三者共用同一判定语义与同一代价模型。
pub async fn probe_config(config: &Config, timeout: Duration) -> Result<AccountHealth> {
    let client =
        ApiClient::new(config.clone()).context("init api client for account health probe")?;
    probe_account_with_timeout(&client, timeout).await
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn healthy_account_is_parsed() {
        let biz = json!({
            "id": "df0b68f1-f8f2-4f2c-860d-98873121e155",
            "email": "xbl*****ht@gmail.com",
            "mobile_number": "13800000000",
            "id_profile": {"name": "Liu", "provider": "x"},
            "chat": {"is_muted": 0, "mute_until": 0, "msg": null}
        });
        let h = parse_health(&biz);
        assert!(!h.muted);
        assert_eq!(h.mute_until, None);
        assert_eq!(h.account_id, "df0b68f1-f8f2-4f2c-860d-98873121e155");
        assert_eq!(h.label, "xbl*****ht@gmail.com");
    }

    #[test]
    fn muted_account_carries_mute_until() {
        let biz = json!({
            "id": "acc-1",
            "email": "a@b.c",
            "chat": {"is_muted": 1, "mute_until": 1794738505.0, "msg": null}
        });
        let h = parse_health(&biz);
        assert!(h.muted);
        assert_eq!(h.mute_until, Some(1794738505));
        assert!(h.mute_until_utc().unwrap().ends_with(" UTC"));
    }

    #[test]
    fn missing_chat_field_degrades_to_healthy() {
        let biz = json!({"id": "acc-2", "email": "x@y.z"});
        let h = parse_health(&biz);
        assert!(!h.muted, "missing chat must degrade to not-muted");
        assert_eq!(h.mute_until, None);
        assert_eq!(h.account_id, "acc-2");
    }

    #[test]
    fn label_priority_is_email_then_mobile_then_name() {
        let email = json!({"id":"a","email":"e@x","mobile_number":"138","id_profile":{"name":"n"}});
        assert_eq!(parse_health(&email).label, "e@x");

        let mobile = json!({"id":"a","mobile_number":"138","id_profile":{"name":"n"}});
        assert_eq!(parse_health(&mobile).label, "138");

        let name = json!({"id":"a","id_profile":{"name":"n"}});
        assert_eq!(parse_health(&name).label, "n");

        let none = json!({"id":"a"});
        assert_eq!(parse_health(&none).label, "a");
    }

    #[test]
    fn empty_label_values_are_skipped() {
        let biz = json!({"id":"a","email":"  ","mobile_number":"","id_profile":{"name":"real"}});
        assert_eq!(parse_health(&biz).label, "real");
    }

    #[test]
    fn is_muted_accepts_bool_and_number() {
        let bool_true = json!({"id":"a","chat":{"is_muted":true}});
        assert!(parse_health(&bool_true).muted);

        let num_one = json!({"id":"a","chat":{"is_muted":1}});
        assert!(parse_health(&num_one).muted);

        let num_zero = json!({"id":"a","chat":{"is_muted":0}});
        assert!(!parse_health(&num_zero).muted);
    }

    #[test]
    fn probe_timeout_is_shorter_than_budget() {
        // interactive login budget 600s
        let t = probe_timeout(Duration::from_secs(600));
        assert!(t < Duration::from_secs(600));
        assert_eq!(t, Duration::from_secs(15));

        // auto-refresh budget 90s
        let t = probe_timeout(Duration::from_secs(90));
        assert!(t < Duration::from_secs(90));
        assert_eq!(t, Duration::from_secs(15));

        // tiny budget: one third of it, still strictly shorter
        let t = probe_timeout(Duration::from_secs(9));
        assert!(t < Duration::from_secs(9));
        assert_eq!(t, Duration::from_secs(3));
    }

    /// 构造指向本地 mock 的测试配置。
    fn test_config(base_url: &str, token: &str) -> Config {
        Config {
            credentials: std::sync::Arc::new(std::sync::RwLock::new(crate::config::Credentials {
                user_token: token.to_string(),
                smid_v2: String::new(),
                cf_clearance: String::new(),
            })),
            pool: std::sync::Arc::new(std::sync::RwLock::new(
                crate::accounts::AccountPool::default(),
            )),
            base_url: base_url.to_string(),
            poll_timeout: Duration::from_secs(60),
            poll_interval: Duration::from_secs(1),
            chat_timeout: Duration::from_secs(120),
            upload_timeout: Duration::from_secs(60),
            max_retries: 3,
            app_version: "test".to_string(),
            client_locale: "zh-CN".to_string(),
            model_type: None,
            upload_min_interval: Duration::from_secs(5),
            auto_refresh: true,
        }
    }

    /// 启动一个本地 mock HTTP 服务，返回 (base_url, 请求行接收端)。
    fn spawn_mock(body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = tx.send(req.lines().next().unwrap_or("").to_string());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}"), rx)
    }

    /// 探针 SHALL 是只读 GET（不发起上传、不取 PoW 挑战）。
    #[tokio::test]
    async fn probe_is_a_read_only_get() {
        let body = r#"{"code":0,"msg":"ok","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"acc-1","email":"a@b.c","chat":{"is_muted":0,"mute_until":0}}}}"#;
        let (base, rx) = spawn_mock(body);
        let config = test_config(&base, "test-token");
        let client = ApiClient::new(config).expect("client");
        let health = probe_account(&client).await.expect("probe");
        assert!(!health.muted);
        assert_eq!(health.account_id, "acc-1");
        assert_eq!(health.label, "a@b.c");

        let req = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("mock should receive the probe request");
        assert!(
            req.starts_with("GET /api/v0/users/current"),
            "probe must be a read-only GET, got: {req}"
        );
    }

    /// 被静音账号 SHALL 被探针识别，并带出截止时间。
    #[tokio::test]
    async fn probe_detects_muted_account() {
        let body = r#"{"code":0,"msg":"ok","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"acc-2","email":"x@y.z","chat":{"is_muted":1,"mute_until":1794738505}}}}"#;
        let (base, _rx) = spawn_mock(body);
        let config = test_config(&base, "test-token");
        let client = ApiClient::new(config).expect("client");
        let health = probe_account(&client).await.expect("probe");
        assert!(health.muted);
        assert_eq!(health.mute_until, Some(1794738505));
    }

    /// token 失效 SHALL 表现为鉴权失败（biz_code=40003 顶层回退）。
    #[tokio::test]
    async fn probe_reports_auth_failure_for_invalid_token() {
        let body = r#"{"code":40003,"msg":"authorization failed","data":null}"#;
        let (base, _rx) = spawn_mock(body);
        let config = test_config(&base, "bad-token");
        let client = ApiClient::new(config).expect("client");
        let err = probe_account(&client).await.expect_err("probe should fail");
        assert!(
            err.downcast_ref::<crate::client::AuthFailure>().is_some(),
            "invalid token must surface as AuthFailure, got: {err}"
        );
    }

    /// 探测超时 SHALL 返回可区分错误，SHALL NOT 静默通过。
    #[tokio::test]
    async fn probe_timeout_is_distinguishable() {
        let body = r#"{"code":0,"data":{"biz_code":0,"biz_data":{"id":"a"}}}"#;
        let (base, _rx) = spawn_mock(body);
        let config = test_config(&base, "test-token");
        let client = ApiClient::new(config).expect("client");
        let err = probe_account_with_timeout(&client, Duration::from_nanos(1))
            .await
            .expect_err("probe should time out");
        assert!(err.to_string().contains("timed out"), "{err}");
    }
}

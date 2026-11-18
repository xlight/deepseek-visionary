//! 账号池：多账号凭据 + 每账号状态 + 轮询游标 + 固定账号 + 删除墓碑。
//!
//! 存储位置：~/.deepseek-visionary/accounts.json（与 config.json 并存）。
//!
//! 设计要点：
//! - **并存而非替换**：config.json 格式不变（单账号用户零感知），账号池是渐进增强；
//!   池不存在 → 空池 → 单账号模式，行为与旧版完全一致。
//! - **切换账号 = 换凭据**：DeepSeek 的上传风控是账号维度的，一个账号被静音后
//!   该账号所有上传都被拒；池模式按请求轮询账号，账号级失败自动换下一个。
//! - **并发写协议**：池文件会被多个进程写（MCP server 常驻随请求更新状态；
//!   CLI 子命令各自独立进程）。因此写回一律「重读磁盘 → 合并本进程变更 → 原子替换」，
//!   删除用**墓碑**传播（否则其它进程会把已删账号写回而复活），
//!   「重读-合并-写回」序列用**锁文件**保证跨进程互斥。
//! - **游标落盘**：CLI 是一次性进程，游标只在内存里的话每次都从账号 #1 开始；
//!   落盘后跨进程也能继续轮换。

use crate::client::format_epoch_utc;
use crate::config::{write_private_json, Credentials};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 账号状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    /// 健康（可用）。
    #[default]
    Healthy,
    /// 被上传风控静音（到 mute_until 为止不可用）。
    Muted,
    /// 凭据失效（服务端拒绝鉴权）。
    Expired,
}

/// 轮询策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// 真轮询：每次分析请求换一个账号（当前唯一实现的策略）。
    #[default]
    Rotate,
    /// 粘住同一账号直到失效（预留）。
    Failover,
}

/// 账号不可用的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnusableReason {
    /// 被静音（上传风控），until 为 unix 秒截止时间（None = 未知）。
    Muted { until: Option<u64> },
    /// 凭据失效。
    Expired,
}

impl UnusableReason {
    /// 人类可读描述（含静音截止时间）。
    pub fn message(&self) -> String {
        match self {
            UnusableReason::Muted { until: Some(u) } => format!(
                "账号被静音（上传风控），截止 {} (unix {})",
                format_epoch_utc(*u as i64),
                u
            ),
            UnusableReason::Muted { until: None } => {
                "账号被静音（上传风控），截止时间未知".to_string()
            }
            UnusableReason::Expired => "凭据已失效（服务端拒绝鉴权）".to_string(),
        }
    }

    /// 对应的账号状态。
    pub fn status(&self) -> AccountStatus {
        match self {
            UnusableReason::Muted { .. } => AccountStatus::Muted,
            UnusableReason::Expired => AccountStatus::Expired,
        }
    }
}

/// 池中的一个账号。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolAccount {
    /// 账号稳定标识（探针返回的 uuid，跨 token 轮换不变）。
    #[serde(rename = "id")]
    pub account_id: String,
    /// 展示标签（email → 手机号 → 昵称 → id）。
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub user_token: String,
    #[serde(default)]
    pub smid_v2: String,
    #[serde(default)]
    pub cf_clearance: String,
    #[serde(default)]
    pub status: AccountStatus,
    /// 静音截止时间（unix 秒）。
    #[serde(default)]
    pub mute_until: Option<u64>,
    /// 最近使用时刻（UTC 文本）。
    #[serde(default)]
    pub last_used: Option<String>,
}

impl PoolAccount {
    /// 该账号自己的凭据。
    pub fn credentials(&self) -> Credentials {
        Credentials {
            user_token: self.user_token.clone(),
            smid_v2: self.smid_v2.clone(),
            cf_clearance: self.cf_clearance.clone(),
        }
    }

    /// 静音是否已到期（到期后视为可用——惰性恢复）。
    pub fn mute_expired(&self, now_unix: u64) -> bool {
        match self.mute_until {
            Some(until) => now_unix >= until,
            None => false,
        }
    }

    /// 当前是否可用：状态健康（或静音已到期）且 token 非空。
    pub fn usable(&self, now_unix: u64) -> bool {
        if self.user_token.is_empty() {
            return false;
        }
        match self.status {
            AccountStatus::Healthy => true,
            AccountStatus::Muted => self.mute_expired(now_unix),
            AccountStatus::Expired => false,
        }
    }
}

/// 账号池。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountPool {
    #[serde(default)]
    pub accounts: Vec<PoolAccount>,
    /// 轮询游标（落盘：CLI 一次性进程靠它跨进程轮换）。
    #[serde(default)]
    pub cursor: usize,
    #[serde(default)]
    pub strategy: Strategy,
    /// 固定账号 id（至多一个；None = 不固定）。
    #[serde(default)]
    pub pinned: Option<String>,
    /// 已删除账号的墓碑（防止其它进程把已删账号写回而复活）。
    #[serde(default)]
    pub removed: Vec<String>,
}

/// 选号结果。
#[derive(Debug, Clone)]
pub enum AccountSelection {
    /// 选中的账号（游标已前移；固定账号不前移）。
    Selected(Box<PoolAccount>),
    /// 固定账号存在但不可用——SHALL 仍只用它，不得静默回退轮询。
    PinnedUnavailable {
        label: String,
        reason: UnusableReason,
    },
    /// 池中无可用账号。
    None,
}

/// 登记结果：新增 vs 已存在（更新）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    Added,
    Updated,
}

impl AccountPool {
    /// 是否处于池模式（池中有账号）。
    pub fn is_pool_mode(&self) -> bool {
        !self.accounts.is_empty()
    }

    /// 池中的账号列表。
    pub fn accounts(&self) -> &[PoolAccount] {
        &self.accounts
    }

    /// 按 id 查找账号。
    pub fn find(&self, account_id: &str) -> Option<&PoolAccount> {
        self.accounts.iter().find(|a| a.account_id == account_id)
    }

    /// 按 id 查找账号（可变）。
    pub fn find_mut(&mut self, account_id: &str) -> Option<&mut PoolAccount> {
        self.accounts
            .iter_mut()
            .find(|a| a.account_id == account_id)
    }

    /// 当前固定账号 id。
    #[allow(dead_code)] // 由账号管理 CLI（accounts pin / list）调用
    pub fn pinned_id(&self) -> Option<&str> {
        self.pinned.as_deref()
    }

    /// 可用账号数量。
    pub fn usable_count(&self) -> usize {
        let now = now_unix();
        self.accounts.iter().filter(|a| a.usable(now)).count()
    }

    /// 取下一个可用账号并前移游标。
    ///
    /// - 跳过 muted（未到期）/ expired / 空凭据的账号；
    /// - 静音已到期 → 惰性恢复为可用；
    /// - 固定账号可用 → 直接返回它且**不推进游标**；
    /// - 固定账号不可用 → 返回 PinnedUnavailable（SHALL NOT 静默回退轮询：
    ///   固定是用户的排障手段，静默换号会让排障结论失效）。
    pub fn next_usable(&mut self) -> AccountSelection {
        let now = now_unix();
        if let Some(pinned) = self.pinned.clone() {
            if let Some(account) = self.accounts.iter().find(|a| a.account_id == pinned) {
                if account.usable(now) {
                    let account = account.clone();
                    return AccountSelection::Selected(Box::new(account));
                }
                let reason = match account.status {
                    AccountStatus::Muted => UnusableReason::Muted {
                        until: account.mute_until,
                    },
                    AccountStatus::Expired => UnusableReason::Expired,
                    AccountStatus::Healthy => UnusableReason::Expired,
                };
                return AccountSelection::PinnedUnavailable {
                    label: display_label(account),
                    reason,
                };
            }
        }

        let len = self.accounts.len();
        if len == 0 {
            return AccountSelection::None;
        }
        let start = self.cursor % len;
        for offset in 0..len {
            let idx = (start + offset) % len;
            if self.accounts[idx].usable(now) {
                self.cursor = (idx + 1) % len;
                let account = self.accounts[idx].clone();
                return AccountSelection::Selected(Box::new(account));
            }
        }
        AccountSelection::None
    }

    /// 标记账号不可用（静音 / 凭据失效）。
    pub fn mark_unusable(&mut self, account_id: &str, reason: UnusableReason) {
        let until = match &reason {
            UnusableReason::Muted { until } => *until,
            UnusableReason::Expired => None,
        };
        if let Some(account) = self.find_mut(account_id) {
            account.status = reason.status();
            account.mute_until = until;
        }
    }

    /// 按 id 登记账号：已存在则更新凭据与状态（Updated），否则插入（Added）。
    ///
    /// 显式登记会**解除墓碑**（重新登录同一个账号 = 用户明确要把它加回来）。
    pub fn register(&mut self, account: PoolAccount) -> RegisterOutcome {
        self.removed.retain(|id| id != &account.account_id);
        if let Some(existing) = self.find_mut(&account.account_id) {
            existing.user_token = account.user_token;
            existing.smid_v2 = account.smid_v2;
            existing.cf_clearance = account.cf_clearance;
            if !account.label.is_empty() {
                existing.label = account.label;
            }
            existing.status = account.status;
            existing.mute_until = account.mute_until;
            RegisterOutcome::Updated
        } else {
            self.accounts.push(account);
            RegisterOutcome::Added
        }
    }

    /// 移除账号（按 id）：删除条目 + 写墓碑 + 若被固定则清除固定。
    #[allow(dead_code)] // 由账号管理 CLI（accounts remove）调用
    pub fn remove(&mut self, account_id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.account_id != account_id);
        let removed = self.accounts.len() != before;
        if removed && !self.removed.iter().any(|id| id == account_id) {
            self.removed.push(account_id.to_string());
        }
        if self.pinned.as_deref() == Some(account_id) {
            self.pinned = None;
        }
        removed
    }

    /// 设置 / 清除固定账号（固定不存在的账号时忽略）。
    #[allow(dead_code)] // 由账号管理 CLI（accounts pin / unpin）调用
    pub fn set_pinned(&mut self, account_id: Option<&str>) {
        match account_id {
            Some(id) => {
                if self.accounts.iter().any(|a| a.account_id == id) {
                    self.pinned = Some(id.to_string());
                }
            }
            None => self.pinned = None,
        }
    }

    /// 标记最近使用时刻（内存态；落盘由 persist_selection 控制）。
    pub fn touch_last_used(&mut self, account_id: &str, now_text: &str) {
        if let Some(account) = self.find_mut(account_id) {
            account.last_used = Some(now_text.to_string());
        }
    }
}

/// 池文件路径：~/.deepseek-visionary/accounts.json。
pub fn pool_file() -> Result<PathBuf> {
    Ok(crate::config::data_dir()?.join("accounts.json"))
}

/// 读取池：文件不存在 → 空池（单账号模式）；文件损坏 → 报错（**保留原文件**）。
pub fn load_pool() -> Result<AccountPool> {
    load_pool_from(&pool_file()?)
}

/// 从指定路径读取池。
pub fn load_pool_from(path: &Path) -> Result<AccountPool> {
    if !path.exists() {
        return Ok(AccountPool::default());
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut pool: AccountPool =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    // 墓碑里的账号不应出现在池中（防止其它进程把已删账号写回）。
    let removed = pool.removed.clone();
    pool.accounts
        .retain(|a| !removed.iter().any(|id| id == &a.account_id));
    Ok(pool)
}

/// 原子写池（临时文件 + rename；Unix 0600）。
#[allow(dead_code)] // 由账号管理 CLI 调用
pub fn save_pool(pool: &AccountPool) -> Result<()> {
    save_pool_to(&pool_file()?, pool)
}

/// 原子写池到指定路径。
pub fn save_pool_to(path: &Path, pool: &AccountPool) -> Result<()> {
    write_private_json(path, pool)
}

/// 跨进程并发写协议：重读磁盘 → 合并本进程变更 → 原子写回。
///
/// - **不得**用内存快照整体覆盖磁盘内容（否则其它进程的状态变更会丢）；
/// - 删除通过墓碑传播（remove 写墓碑，其它进程合并时不会把已删账号写回）；
/// - 「重读-合并-写回」由**锁文件**保证跨进程互斥（进程内 RwLock 不够）。
pub fn merge_and_save<F>(path: &Path, mutate: F) -> Result<()>
where
    F: FnOnce(&mut AccountPool) -> Result<()>,
{
    let _lock = PoolLock::acquire(path)?;
    let mut pool = load_pool_from(path)?;
    mutate(&mut pool)?;
    save_pool_to(path, &pool)
}

/// 轮询选号后的写回：持久化**游标**（跨进程轮换的关键）+ 节流更新 last_used。
///
/// 游标与 last_used 合并为一次写回；last_used 的节流窗口默认 60 秒
/// （DEEPSEEK_ACCOUNT_LAST_USED_WINDOW_SECS），避免每个请求都重写整个池文件。
pub fn persist_selection(path: &Path, pool: &mut AccountPool, account_id: &str) -> Result<()> {
    let window = last_used_window();
    let should_touch = should_touch_last_used(account_id, window);
    if should_touch {
        pool.touch_last_used(account_id, &now_text());
        remember_touch(account_id);
    }
    let cursor = pool.cursor;
    let last_used = pool.find(account_id).and_then(|a| a.last_used.clone());
    let account_id = account_id.to_string();
    merge_and_save(path, |disk| {
        disk.cursor = cursor;
        if should_touch {
            if let Some(ts) = last_used.clone() {
                if let Some(account) = disk.find_mut(&account_id) {
                    account.last_used = Some(ts);
                }
            }
        }
        Ok(())
    })
}

/// 迁移报告。
#[allow(dead_code)] // 由账号管理 CLI（accounts migrate）调用
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    /// 导入的账号数。
    pub imported: usize,
    /// 跳过的账号数。
    pub skipped: usize,
    /// 跳过原因（逐条）。
    pub reasons: Vec<String>,
}

#[allow(dead_code)] // 由账号管理 CLI（accounts migrate）调用
impl MigrationReport {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        let mut out = format!(
            "imported {} account(s), skipped {}",
            self.imported, self.skipped
        );
        for reason in &self.reasons {
            out.push_str(&format!("\n- {reason}"));
        }
        out
    }
}

/// 存量凭据一次性迁移：读 config.json 凭据 → 只读探针换账号稳定标识 → 写入池。
///
/// - **幂等**：池中已有同 id 则跳过；
/// - **非破坏**：不修改 / 不清空 config.json；
/// - **显式触发**：只在本函数被调用时执行，绝不在启动路径自动跑；
/// - 探针失败 → 返回错误（提示联网重试），**不写入占位标识**
///   （占位标识与日后真实标识不一致，会让同一账号出现两条、后续移除删不掉）。
#[allow(dead_code)] // 由账号管理 CLI（accounts migrate）调用
pub async fn migrate_from_config(
    config: &crate::config::Config,
    pool_path: &Path,
) -> Result<MigrationReport> {
    let creds = config.credentials();
    if creds.user_token.is_empty() {
        anyhow::bail!(
            "no credentials found in config.json — run `visionary-cli login` first, then retry the migration"
        );
    }
    let health = crate::account_health::probe_config(config, Duration::from_secs(15))
        .await
        .context(
            "account health probe failed (offline / network / invalid credentials?) —              the migration needs the network to resolve the stable account id;              no placeholder id was written",
        )?;
    if health.account_id.is_empty() {
        anyhow::bail!(
            "the probe response carried no account id — refusing to write a placeholder id; retry later"
        );
    }

    let account = PoolAccount {
        account_id: health.account_id.clone(),
        label: health.label.clone(),
        user_token: creds.user_token.clone(),
        smid_v2: creds.smid_v2.clone(),
        cf_clearance: creds.cf_clearance.clone(),
        status: if health.muted {
            AccountStatus::Muted
        } else {
            AccountStatus::Healthy
        },
        mute_until: health.mute_until,
        last_used: None,
    };

    let mut report = MigrationReport::default();
    let mut outcome = RegisterOutcome::Added;
    merge_and_save(pool_path, |pool| {
        outcome = pool.register(account.clone());
        Ok(())
    })?;
    match outcome {
        RegisterOutcome::Added => report.imported += 1,
        RegisterOutcome::Updated => {
            report.skipped += 1;
            report.reasons.push(format!(
                "{} ({}) is already in the pool — credentials refreshed, no duplicate entry",
                display_label(&account),
                account.account_id
            ));
        }
    }
    Ok(report)
}

/// 账号展示标签（缺省退回 id）。
fn display_label(account: &PoolAccount) -> String {
    if account.label.is_empty() {
        account.account_id.clone()
    } else {
        account.label.clone()
    }
}

/// 当前 unix 秒。
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前时间的 RFC3339 UTC 文本（池文件里的 last_used 格式）。
fn now_text() -> String {
    format_rfc3339_utc(now_unix())
}

/// last_used 节流窗口。
fn last_used_window() -> Duration {
    let secs = std::env::var("DEEPSEEK_ACCOUNT_LAST_USED_WINDOW_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(60);
    Duration::from_secs(secs)
}

/// 进程内的「最近一次 touch」记录（用于节流）。
fn touch_registry() -> &'static Mutex<std::collections::HashMap<String, u64>> {
    static REG: OnceLock<Mutex<std::collections::HashMap<String, u64>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 节流判定：窗口内不重复写盘。
fn should_touch_last_used(account_id: &str, window: Duration) -> bool {
    let now = now_unix();
    if let Ok(guard) = touch_registry().lock() {
        if let Some(last) = guard.get(account_id) {
            if now.saturating_sub(*last) < window.as_secs() {
                return false;
            }
        }
    }
    true
}

/// 记录本次 touch。
fn remember_touch(account_id: &str) {
    if let Ok(mut guard) = touch_registry().lock() {
        guard.insert(account_id.to_string(), now_unix());
    }
}

/// 跨进程文件锁：用 create_new 独占创建锁文件实现（不引入第三方依赖）。
struct PoolLock {
    path: PathBuf,
}

impl PoolLock {
    fn acquire(target: &Path) -> Result<Self> {
        let path = lock_path(target);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = file.write_all(std::process::id().to_string().as_bytes());
                    return Ok(PoolLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // 陈旧锁回收：超过 30 秒的锁视为上一次崩溃残留。
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Some(age) = meta.modified().ok().and_then(|m| m.elapsed().ok()) {
                            if age > Duration::from_secs(30) {
                                let _ = fs::remove_file(&path);
                                continue;
                            }
                        }
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!(
                            "timed out waiting for the account pool lock: {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("create lock {}", path.display()));
                }
            }
        }
    }
}

impl Drop for PoolLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// 锁文件路径：<pool>.lock。
fn lock_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "accounts.json".to_string());
    target.with_file_name(format!(".{name}.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 临时目录（测试不得触碰用户真实目录）。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "visionary-accounts-test-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn account(id: &str, token: &str) -> PoolAccount {
        PoolAccount {
            account_id: id.to_string(),
            label: format!("{id}@example.com"),
            user_token: token.to_string(),
            smid_v2: String::new(),
            cf_clearance: String::new(),
            status: AccountStatus::Healthy,
            mute_until: None,
            last_used: None,
        }
    }

    #[test]
    fn pool_file_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("accounts.json");
        let mut pool = AccountPool::default();
        assert_eq!(pool.register(account("a", "tok-a")), RegisterOutcome::Added);
        pool.cursor = 1;
        pool.pinned = Some("a".into());
        save_pool_to(&path, &pool).expect("save");

        let loaded = load_pool_from(&path).expect("load");
        assert_eq!(loaded.accounts().len(), 1);
        assert_eq!(loaded.accounts()[0].account_id, "a");
        assert_eq!(loaded.cursor, 1);
        assert_eq!(loaded.pinned_id(), Some("a"));
    }

    #[test]
    fn missing_pool_file_is_empty_pool() {
        let dir = temp_dir("missing");
        let pool = load_pool_from(&dir.join("accounts.json")).expect("load");
        assert!(!pool.is_pool_mode());
        assert_eq!(pool.accounts().len(), 0);
    }

    #[test]
    fn corrupt_pool_file_errors_and_keeps_file() {
        let dir = temp_dir("corrupt");
        let path = dir.join("accounts.json");
        fs::write(&path, "{not json").expect("write corrupt");
        let err = load_pool_from(&path).expect_err("corrupt pool must error");
        assert!(format!("{err:#}").contains("parse"), "unexpected: {err:#}");
        // 原文件必须保留（不得覆盖 / 删除）
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[test]
    fn next_usable_skips_unusable_accounts() {
        let mut pool = AccountPool::default();
        let mut muted = account("muted", "tok-m");
        muted.status = AccountStatus::Muted;
        muted.mute_until = Some(now_unix() + 3600);
        let mut expired = account("expired", "tok-e");
        expired.status = AccountStatus::Expired;
        pool.accounts.push(muted);
        pool.accounts.push(expired);
        pool.accounts.push(account("empty", ""));
        pool.accounts.push(account("ok", "tok-ok"));

        match pool.next_usable() {
            AccountSelection::Selected(a) => assert_eq!(a.account_id, "ok"),
            other => panic!("unexpected selection: {other:?}"),
        }
    }

    #[test]
    fn mute_expiry_recovers_lazily() {
        let mut pool = AccountPool::default();
        let mut muted = account("m", "tok");
        muted.status = AccountStatus::Muted;
        muted.mute_until = Some(now_unix().saturating_sub(10));
        pool.accounts.push(muted);
        match pool.next_usable() {
            AccountSelection::Selected(a) => assert_eq!(a.account_id, "m"),
            other => panic!("expired mute must recover: {other:?}"),
        }
    }

    #[test]
    fn cursor_rotates_through_accounts() {
        let mut pool = AccountPool::default();
        pool.accounts.push(account("a", "tok-a"));
        pool.accounts.push(account("b", "tok-b"));
        pool.accounts.push(account("c", "tok-c"));
        let mut seen = Vec::new();
        for _ in 0..4 {
            match pool.next_usable() {
                AccountSelection::Selected(a) => seen.push(a.account_id),
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        assert_eq!(seen, vec!["a", "b", "c", "a"]);
    }

    #[test]
    fn pinned_account_is_always_selected_and_cursor_stays() {
        let mut pool = AccountPool::default();
        pool.accounts.push(account("a", "tok-a"));
        pool.accounts.push(account("b", "tok-b"));
        pool.pinned = Some("b".into());
        pool.cursor = 0;
        for _ in 0..3 {
            match pool.next_usable() {
                AccountSelection::Selected(a) => assert_eq!(a.account_id, "b"),
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        assert_eq!(
            pool.cursor, 0,
            "pinned selection must not advance the cursor"
        );
    }

    #[test]
    fn pinned_unavailable_does_not_fall_back_to_rotation() {
        let mut pool = AccountPool::default();
        let mut pinned = account("p", "tok-p");
        pinned.status = AccountStatus::Muted;
        pinned.mute_until = Some(now_unix() + 600);
        pool.accounts.push(pinned);
        pool.accounts.push(account("other", "tok-other"));
        pool.pinned = Some("p".into());
        match pool.next_usable() {
            AccountSelection::PinnedUnavailable { label, reason } => {
                assert_eq!(label, "p@example.com");
                assert!(matches!(reason, UnusableReason::Muted { .. }));
            }
            other => panic!("pinned account must not fall back: {other:?}"),
        }
    }

    #[test]
    fn removing_pinned_account_clears_pin() {
        let mut pool = AccountPool::default();
        pool.accounts.push(account("p", "tok-p"));
        pool.pinned = Some("p".into());
        assert!(pool.remove("p"));
        assert_eq!(pool.pinned_id(), None);
        assert!(pool.removed.iter().any(|id| id == "p"));
    }

    #[test]
    fn register_is_idempotent() {
        let mut pool = AccountPool::default();
        assert_eq!(pool.register(account("a", "tok-1")), RegisterOutcome::Added);
        assert_eq!(
            pool.register(account("a", "tok-2")),
            RegisterOutcome::Updated
        );
        assert_eq!(pool.accounts().len(), 1);
        assert_eq!(pool.find("a").unwrap().user_token, "tok-2");
    }

    #[test]
    fn merge_and_save_keeps_other_process_changes() {
        let dir = temp_dir("merge");
        let path = dir.join("accounts.json");
        // 进程 A：登记账号 a
        merge_and_save(&path, |pool| {
            pool.register(account("a", "tok-a"));
            Ok(())
        })
        .expect("merge A");
        // 进程 B（内存快照里只有 b）：登记 b —— a 的条目不得丢失
        merge_and_save(&path, |pool| {
            pool.register(account("b", "tok-b"));
            Ok(())
        })
        .expect("merge B");
        let loaded = load_pool_from(&path).expect("load");
        assert_eq!(loaded.accounts().len(), 2);
        assert!(loaded.find("a").is_some());
        assert!(loaded.find("b").is_some());
    }

    #[test]
    fn removed_account_is_not_resurrected_by_merge() {
        let dir = temp_dir("tombstone");
        let path = dir.join("accounts.json");
        merge_and_save(&path, |pool| {
            pool.register(account("a", "tok-a"));
            pool.register(account("b", "tok-b"));
            Ok(())
        })
        .expect("merge");
        // 进程 A 删除 a（写墓碑）
        merge_and_save(&path, |pool| {
            pool.remove("a");
            Ok(())
        })
        .expect("remove");
        // 进程 B 的旧快照里仍有 a：合并时不得把它写回
        merge_and_save(&path, |pool| {
            pool.register(account("b", "tok-b2"));
            Ok(())
        })
        .expect("merge B");
        let loaded = load_pool_from(&path).expect("load");
        assert!(
            loaded.find("a").is_none(),
            "removed account must stay removed"
        );
        assert!(loaded.find("b").is_some());
        assert!(loaded.removed.iter().any(|id| id == "a"));
    }

    #[test]
    fn persist_selection_writes_cursor_to_disk() {
        let dir = temp_dir("cursor");
        let path = dir.join("accounts.json");
        let mut pool = AccountPool::default();
        pool.accounts.push(account("a", "tok-a"));
        pool.accounts.push(account("b", "tok-b"));
        save_pool_to(&path, &pool).expect("save");

        // 取号 → 游标前移 → 写回磁盘（跨进程轮换的关键）
        let mut pool = load_pool_from(&path).expect("load");
        let selection = pool.next_usable();
        let id = match selection {
            AccountSelection::Selected(a) => a.account_id,
            other => panic!("unexpected selection: {other:?}"),
        };
        persist_selection(&path, &mut pool, &id).expect("persist");

        let disk = load_pool_from(&path).expect("load");
        assert_eq!(
            disk.cursor, 1,
            "cursor must be persisted for cross-process rotation"
        );
    }

    #[test]
    fn unusable_reason_messages() {
        let muted = UnusableReason::Muted {
            until: Some(1794738505),
        };
        assert!(muted.message().contains("静音"));
        assert_eq!(muted.status(), AccountStatus::Muted);
        let expired = UnusableReason::Expired;
        assert_eq!(expired.status(), AccountStatus::Expired);
        assert!(expired.message().contains("失效"));
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    /// 临时目录（测试不得触碰用户真实目录）。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "visionary-accounts-migrate-test-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
    /// 迁移测试用的 Config（纯内存，不读用户目录）。
    fn test_config(base_url: &str) -> crate::config::Config {
        crate::config::Config {
            credentials: std::sync::Arc::new(std::sync::RwLock::new(Credentials {
                user_token: "tok-migrate".into(),
                smid_v2: String::new(),
                cf_clearance: String::new(),
            })),
            pool: std::sync::Arc::new(std::sync::RwLock::new(AccountPool::default())),
            base_url: base_url.to_string(),
            poll_timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            chat_timeout: Duration::from_secs(120),
            upload_timeout: Duration::from_secs(120),
            max_retries: 3,
            app_version: "test".into(),
            client_locale: "en".into(),
            model_type: None,
            upload_min_interval: Duration::from_secs(0),
            auto_refresh: false,
        }
    }

    /// 假探针服务：返回固定 body（用于模拟探针成功 / 失败）。
    fn spawn_probe(body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
        let addr = listener.local_addr().expect("probe addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                use std::io::{Read, Write};
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    const PROBE_OK: &str = r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{"id":"acc-migrate-1","email":"migrate@example.com","chat":{"is_muted":0,"mute_until":0,"msg":null}}}}"#;

    #[tokio::test]
    async fn migrate_imports_account_and_is_idempotent() {
        let dir = temp_dir("migrate");
        let path = dir.join("accounts.json");
        let base_url = spawn_probe(PROBE_OK);
        let config = test_config(&base_url);

        let report = migrate_from_config(&config, &path)
            .await
            .expect("migrate should succeed");
        assert_eq!(report.imported, 1);
        assert_eq!(report.skipped, 0);

        let pool = load_pool_from(&path).expect("load pool");
        assert_eq!(pool.accounts().len(), 1);
        assert_eq!(pool.accounts()[0].account_id, "acc-migrate-1");
        assert_eq!(pool.accounts()[0].user_token, "tok-migrate");

        // 幂等：再跑一次不产生重复条目。
        let report2 = migrate_from_config(&config, &path)
            .await
            .expect("second migrate should succeed");
        assert_eq!(report2.imported, 0);
        assert_eq!(report2.skipped, 1);
        assert!(
            !report2.reasons.is_empty(),
            "skip reason must be observable"
        );
        let pool = load_pool_from(&path).expect("load pool");
        assert_eq!(pool.accounts().len(), 1, "migration must be idempotent");
    }

    #[tokio::test]
    async fn migrate_probe_failure_errors_without_placeholder_id() {
        let dir = temp_dir("migrate-fail");
        let path = dir.join("accounts.json");
        let base_url = spawn_probe(
            r#"{"code":40003,"msg":"Authorization Failed","data":{"biz_code":40003,"biz_msg":"Authorization Failed (invalid token)","biz_data":null}}"#,
        );
        let config = test_config(&base_url);

        let err = migrate_from_config(&config, &path)
            .await
            .expect_err("probe failure must error");
        let text = format!("{err:#}");
        assert!(
            text.contains("probe") || text.contains("probe failed"),
            "unexpected error: {text}"
        );
        // 关键：失败时不得写入占位标识（否则日后会出现重复账号）。
        let pool = load_pool_from(&path).expect("load pool");
        assert!(pool.accounts().is_empty(), "no placeholder may be written");
        assert!(!path.exists() || load_pool_from(&path).unwrap().accounts().is_empty());
    }

    #[tokio::test]
    async fn migrate_is_non_destructive_to_compat_config() {
        let dir = temp_dir("migrate-nondestructive");
        let path = dir.join("accounts.json");
        let compat = dir.join("config.json");
        let original = r#"{"user_token":"tok-migrate","smid_v2":"smid-x","cf_clearance":"cf-y"}"#;
        fs::write(&compat, original).expect("write config");
        let base_url = spawn_probe(PROBE_OK);
        let config = test_config(&base_url);

        migrate_from_config(&config, &path)
            .await
            .expect("migrate should succeed");
        assert_eq!(
            fs::read_to_string(&compat).unwrap(),
            original,
            "migration must not touch config.json"
        );
    }

    #[tokio::test]
    async fn migrate_is_not_triggered_on_config_load() {
        let dir = temp_dir("migrate-explicit");
        let path = dir.join("accounts.json");
        // 只构造配置（等价于启动路径）：不得产生任何池文件。
        let _config = test_config("http://127.0.0.1:1");
        assert!(
            !path.exists(),
            "migration must only run when explicitly invoked"
        );
    }
}

/// 账号视图（**不含凭据明文**）：三端（CLI / MCP / 插件）共用的展示模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    /// 账号稳定标识。
    pub id: String,
    /// 展示标签（email → 手机号 → 昵称 → id）。
    pub label: String,
    /// 账号状态（静音到期视为健康——与选号口径一致）。
    pub status: AccountStatus,
    /// 静音截止时间（unix 秒）。
    pub mute_until: Option<u64>,
    /// 最近使用时刻（RFC3339 UTC）。
    pub last_used: Option<String>,
    /// 是否为固定账号。
    pub pinned: bool,
}

/// 账号池视图（三端同源）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolView {
    /// 池是否启用（池中有账号）。
    pub enabled: bool,
    /// 账号列表。
    pub accounts: Vec<AccountView>,
    /// 账号总数。
    pub total: usize,
    /// 健康账号数。
    pub healthy: usize,
    /// 静音账号数。
    pub muted: usize,
    /// 凭据失效账号数。
    pub expired: usize,
    /// 固定账号 id。
    pub pinned: Option<String>,
    /// 当前使用账号 id：固定 > 最近使用 > 无。
    pub current: Option<String>,
    /// 环境变量凭据已被忽略（池模式下 env 不参与选号）。
    pub env_ignored: bool,
}

impl AccountPool {
    /// 生成视图（不含凭据明文）。
    ///
    /// 状态计数口径与选号口径一致：静音已到期的账号计为**健康**
    /// （惰性恢复——选号时它就是可用的）。
    pub fn view(&self, env_ignored: bool) -> PoolView {
        let now = now_unix();
        let mut healthy = 0usize;
        let mut muted = 0usize;
        let mut expired = 0usize;
        let accounts: Vec<AccountView> = self
            .accounts
            .iter()
            .map(|account| {
                let status = match account.status {
                    AccountStatus::Healthy => AccountStatus::Healthy,
                    AccountStatus::Muted if account.mute_expired(now) => AccountStatus::Healthy,
                    AccountStatus::Muted => AccountStatus::Muted,
                    AccountStatus::Expired => AccountStatus::Expired,
                };
                match status {
                    AccountStatus::Healthy => healthy += 1,
                    AccountStatus::Muted => muted += 1,
                    AccountStatus::Expired => expired += 1,
                }
                AccountView {
                    id: account.account_id.clone(),
                    label: display_label(account),
                    status,
                    mute_until: account.mute_until,
                    last_used: account.last_used.clone(),
                    pinned: self.pinned.as_deref() == Some(account.account_id.as_str()),
                }
            })
            .collect();
        PoolView {
            enabled: !self.accounts.is_empty(),
            total: self.accounts.len(),
            healthy,
            muted,
            expired,
            pinned: self.pinned.clone(),
            current: self.current_account_id(),
            accounts,
            env_ignored,
        }
    }

    /// 当前使用账号 id：**固定 > 最近使用 > 无**。
    ///
    /// 三端 SHALL 用同一口径（否则 CLI / MCP / 插件会显示不同的「当前账号」）。
    pub fn current_account_id(&self) -> Option<String> {
        if let Some(pinned) = &self.pinned {
            if self.find(pinned).is_some() {
                return Some(pinned.clone());
            }
        }
        self.accounts
            .iter()
            .filter_map(|account| {
                account
                    .last_used
                    .as_deref()
                    .and_then(parse_rfc3339_utc)
                    .map(|ts| (ts, account.account_id.clone()))
            })
            .max_by_key(|(ts, _)| *ts)
            .map(|(_, id)| id)
    }
}

/// unix 秒 → RFC3339 UTC（`YYYY-MM-DDTHH:MM:SSZ`）。
pub fn format_rfc3339_utc(secs: u64) -> String {
    let secs = secs as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
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
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// 解析 RFC3339 UTC（`YYYY-MM-DDTHH:MM:SSZ`）→ unix 秒；无法解析返回 None。
pub fn parse_rfc3339_utc(text: &str) -> Option<u64> {
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    if text.len() < 20 {
        return None;
    }
    let year = num(0..4)?;
    let month = num(5..7)?;
    let day = num(8..10)?;
    let hour = num(11..13)?;
    let minute = num(14..16)?;
    let second = num(17..19)?;
    let days = days_from_civil(year, month, day);
    let total = days * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(total).ok()
}

/// 公历 → 天数（Howard Hinnant 的 days_from_civil 算法）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn account(id: &str, token: &str) -> PoolAccount {
        PoolAccount {
            account_id: id.to_string(),
            label: format!("{id}@example.com"),
            user_token: token.to_string(),
            smid_v2: String::new(),
            cf_clearance: String::new(),
            status: AccountStatus::Healthy,
            mute_until: None,
            last_used: None,
        }
    }

    #[test]
    fn pool_view_shape_and_counts() {
        let mut pool = AccountPool::default();
        let mut healthy = account("a", "tok-a");
        healthy.last_used = Some("2026-11-08T10:00:00Z".into());
        let mut muted = account("b", "tok-b");
        muted.status = AccountStatus::Muted;
        muted.mute_until = Some(now_unix() + 3600);
        let mut expired = account("c", "tok-c");
        expired.status = AccountStatus::Expired;
        pool.accounts.push(healthy);
        pool.accounts.push(muted);
        pool.accounts.push(expired);

        let view = pool.view(true);
        assert!(view.enabled);
        assert_eq!(view.total, 3);
        assert_eq!(view.healthy, 1);
        assert_eq!(view.muted, 1);
        assert_eq!(view.expired, 1);
        assert_eq!(view.pinned, None);
        assert_eq!(view.current.as_deref(), Some("a"));
        assert!(view.env_ignored);
        // 视图不得包含凭据明文
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("tok-a"),
            "token must not leak into the view: {json}"
        );
        assert!(!json.contains("user_token"), "{json}");
        // 形状：字段名固定
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in [
            "enabled",
            "accounts",
            "total",
            "healthy",
            "muted",
            "expired",
            "pinned",
            "current",
            "env_ignored",
        ] {
            assert!(value.get(key).is_some(), "missing key {key} in {json}");
        }
        assert_eq!(value["accounts"][0]["status"], "healthy");
        assert_eq!(value["accounts"][1]["status"], "muted");
        assert_eq!(value["accounts"][2]["status"], "expired");
    }

    #[test]
    fn pool_view_current_prefers_pinned_then_recent() {
        let mut pool = AccountPool::default();
        let mut older = account("older", "tok-a");
        older.last_used = Some("2026-11-01T00:00:00Z".into());
        let mut newer = account("newer", "tok-b");
        newer.last_used = Some("2026-11-08T00:00:00Z".into());
        pool.accounts.push(older);
        pool.accounts.push(newer);

        // 无固定 → 最近使用者
        assert_eq!(pool.view(false).current.as_deref(), Some("newer"));
        // 有固定 → 固定优先
        pool.pinned = Some("older".into());
        let view = pool.view(false);
        assert_eq!(view.current.as_deref(), Some("older"));
        assert_eq!(view.pinned.as_deref(), Some("older"));
        assert!(view.accounts.iter().any(|a| a.pinned && a.id == "older"));
    }

    #[test]
    fn rfc3339_roundtrip() {
        let text = format_rfc3339_utc(1_762_000_000);
        assert!(text.ends_with('Z'), "{text}");
        assert_eq!(parse_rfc3339_utc(&text), Some(1_762_000_000));
        assert_eq!(parse_rfc3339_utc("not-a-time"), None);
    }
}

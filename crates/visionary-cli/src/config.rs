//! 配置管理：环境变量 + `~/.deepseek-visionary/config.json`（凭据）+ `settings.json`（偏好）。
//!
//! 对照 Python 版 `config.py`，并落实 design.md 决策 6：
//! - 键名对齐 Python：`user_token` / `smid_v2` / `cf_clearance`
//! - 环境变量覆盖：`DEEPSEEK_USER_TOKEN` / `DEEPSEEK_SMIDV2` / `DEEPSEEK_CF_CLEARANCE`
//! - 配置文件写入权限 0600
//! - `RwLock` 保护 + 热重载原子替换（login 工具写入后无需重启）
//! - 偏好配置（如 `model_type`）独立存放于 `settings.json`，不写入 config.json——
//!   后者是凭据文件，`save_credentials` / logout 会整体覆盖（见 design D1）

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

/// 数据目录：`~/.deepseek-visionary/`，存放 config.json 与 session.json。
///
/// 用户目录解析走 `onboarding::home_dir()`（Unix: HOME，Windows: USERPROFILE），
/// 保证无 `HOME` 环境变量的 Windows PowerShell/cmd 环境下同样可用。
pub(crate) fn data_dir() -> Result<PathBuf> {
    let home = crate::onboarding::home_dir()?;
    Ok(home.join(".deepseek-visionary"))
}

/// 配置文件路径：`~/.deepseek-visionary/config.json`。
pub fn config_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("config.json"))
}

/// 偏好配置文件路径：`~/.deepseek-visionary/settings.json`。
///
/// 独立于 config.json（凭据文件，login/logout 全量覆盖）；偏好配置放这里
/// 不会被 login/logout 抹掉（design D1）。
pub fn settings_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

/// 偏好配置（settings.json 内容）。字段缺失即默认，容错容错读取。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SettingsFile {
    /// 上传管道：`None`/`"vision"` → vision 管道；`"ocr"` → OCR 管道。
    #[serde(default)]
    pub model_type: Option<String>,
}

/// modelType 合法值。`"ocr"` 上传时不携带 `x-model-type`（走服务端 OCR 管道）；
/// `None`/`"vision"` 携带 `x-model-type: vision`。
pub const MODEL_TYPE_VISION: &str = "vision";
pub const MODEL_TYPE_OCR: &str = "ocr";

/// 校验 modelType 值是否合法（仅 vision / ocr）。
pub fn validate_model_type(value: &str) -> Result<()> {
    if value == MODEL_TYPE_VISION || value == MODEL_TYPE_OCR {
        Ok(())
    } else {
        anyhow::bail!(
            "invalid model_type {value:?}: must be \"vision\" or \"ocr\" (the server rejects other values such as \"ocr\"-typed headers with HTTP 500)"
        )
    }
}

/// 读取 settings.json（不存在或损坏时返回 Ok(None)）。
pub fn read_settings_file() -> Result<Option<SettingsFile>> {
    let path = settings_file()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let settings =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(settings))
}

/// 原子写入 settings.json（0600）。当前无写入调用方（modelType 经 env / 手动编辑
/// settings.json / 插件面板生效），保留供未来 CLI 配置子命令使用。
#[allow(dead_code)]
pub fn write_settings_file(settings: &SettingsFile) -> Result<()> {
    write_private_json(&settings_file()?, settings)
}

/// 会话文件路径：`~/.deepseek-visionary/session.json`（对应 Python `_get_session_file`）。
pub fn session_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("session.json"))
}

/// 登录浏览器 profile 目录：`~/.deepseek-visionary/browser/`（权限 0700）。
pub fn browser_profile_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("browser"))
}

/// 持久化凭据（config.json 内容）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Credentials {
    #[serde(default)]
    pub user_token: String,
    #[serde(default)]
    pub smid_v2: String,
    #[serde(default)]
    pub cf_clearance: String,
}

/// 应用配置。`credentials` 由 RwLock 保护，支持 login 后热重载原子替换。
#[derive(Debug, Clone)]
pub struct Config {
    pub credentials: std::sync::Arc<RwLock<Credentials>>,
    /// 账号池（`~/.deepseek-visionary/accounts.json`）。与 `credentials` 同构：
    /// `Config` 是 derive(Clone)，克隆共享同一个 Arc。
    /// 池为空 → 单账号模式（行为与旧版一致）；池非空 → 按池轮询。
    pub pool: std::sync::Arc<RwLock<crate::accounts::AccountPool>>,
    pub base_url: String,
    pub poll_timeout: Duration,
    pub poll_interval: Duration,
    pub chat_timeout: Duration,
    pub upload_timeout: Duration,
    pub max_retries: u32,
    pub app_version: String,
    pub client_locale: String,
    /// 上传管道：`None`（默认 vision）或 `"ocr"`。
    /// 优先级：CLI `--model-type` flag（cmd_vision 覆盖）> env > settings.json > 默认。
    pub model_type: Option<String>,
    /// 上传最小间隔（限速 / 风控保护）：两次上传开始之间的最小间隔。
    /// 默认 5 秒；`DEEPSEEK_UPLOAD_MIN_INTERVAL` 覆盖，设为 0 关闭限速。
    pub upload_min_interval: Duration,
    /// token 自动续期开关（默认开启，`DEEPSEEK_AUTO_REFRESH=0` 关闭）。
    pub auto_refresh: bool,
}

impl Config {
    /// 从环境变量 + 配置文件加载。
    ///
    /// 优先级：环境变量 > config.json（对照 Python `Config.from_env`：
    /// token 先查环境变量，为空再读文件；smid/cf 只读环境变量，这里扩展为
    /// 也回退到文件——见 design.md 决策 6 的键名对齐）。
    pub fn load() -> Result<Self> {
        let file_creds = Self::read_credentials_file()
            .ok()
            .flatten()
            .unwrap_or_default();
        let credentials = Credentials {
            user_token: env::var("DEEPSEEK_USER_TOKEN")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or(file_creds.user_token),
            smid_v2: env::var("DEEPSEEK_SMIDV2")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or(file_creds.smid_v2),
            cf_clearance: env::var("DEEPSEEK_CF_CLEARANCE")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or(file_creds.cf_clearance),
        };
        // modelType 优先级：env `DEEPSEEK_VISIONARY_MODEL_TYPE` > settings.json `model_type` > 默认 None(vision)
        let file_settings = read_settings_file().ok().flatten().unwrap_or_default();
        let model_type: Option<String> = match env::var("DEEPSEEK_VISIONARY_MODEL_TYPE")
            .ok()
            .filter(|s| !s.is_empty())
        {
            Some(v) => Some(v),
            None => file_settings.model_type,
        };
        if let Some(mt) = &model_type {
            validate_model_type(mt)?;
        }
        // 账号池：文件不存在 → 空池（单账号模式）；损坏 → 报错并保留原文件
        // （池损坏不能静默吞掉：否则用户会以为还在轮询，实际用的是旧账号）。
        let pool = crate::accounts::load_pool()?;
        Ok(Self {
            credentials: std::sync::Arc::new(RwLock::new(credentials)),
            pool: std::sync::Arc::new(RwLock::new(pool)),
            base_url: env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://chat.deepseek.com".into()),
            poll_timeout: Duration::from_secs(env_u64("DEEPSEEK_POLL_TIMEOUT", 60)),
            poll_interval: Duration::from_millis(
                (env_f64("DEEPSEEK_POLL_INTERVAL", 1.0) * 1000.0) as u64,
            ),
            chat_timeout: Duration::from_secs(env_u64("DEEPSEEK_CHAT_TIMEOUT", 120)),
            upload_timeout: Duration::from_secs(env_u64("DEEPSEEK_UPLOAD_TIMEOUT", 60)),
            max_retries: env_u64("DEEPSEEK_MAX_RETRIES", 3) as u32,
            app_version: "2.0.0".into(),
            client_locale: "zh_CN".into(),
            model_type,
            upload_min_interval: Duration::from_millis(
                (env_f64("DEEPSEEK_UPLOAD_MIN_INTERVAL", 5.0) * 1000.0) as u64,
            ),
            auto_refresh: crate::auth::auto_refresh_enabled(),
        })
    }

    /// 读取 config.json（不存在或损坏时返回 Ok(None)）。
    pub fn read_credentials_file() -> Result<Option<Credentials>> {
        let path = config_file()?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let creds =
            serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        Ok(Some(creds))
    }

    /// 原子写入 config.json（0600）。login 工具与手动兜底路径共用。
    pub fn save_credentials(&self, creds: &Credentials) -> Result<()> {
        let path = config_file()?;
        write_private_json(&path, creds)?;
        // 热重载：原子替换 RwLock 内容
        *self
            .credentials
            .write()
            .map_err(|e| anyhow::anyhow!("credentials lock poisoned: {e}"))? = creds.clone();
        Ok(())
    }

    /// 用**独立的凭据容器**构造一份配置副本。
    ///
    /// 用于「用候选凭据探测账号健康」：候选 token 尚未写入配置，
    /// 若沿用当前凭据，探测判定会属于上一个账号（首次登录则是空 token → 误报鉴权失败）。
    ///
    /// SHARD NOT 直接用 `self.clone()`：`Config` 是 derive(Clone)，克隆共享**同一个** `Arc`，
    /// 写入副本会连带改掉正在生效的凭据。这里克隆后**替换** `credentials` 字段为新的 `Arc`，
    /// 原配置不受影响。
    pub fn with_credentials(&self, creds: Credentials) -> Config {
        let mut copy = self.clone();
        copy.credentials = std::sync::Arc::new(RwLock::new(creds));
        copy
    }
    /// 热重载：**仅替换内存中的凭据**，SHALL NOT 写磁盘。
    ///
    /// 两种用途：
    /// - 账号池轮询期的**内存热替换**（每次分析请求换账号）：只改 RwLock，
    ///   不写 `config.json`（写盘是「登记写回」的职责，不是轮询的）；
    /// - 外部已写入磁盘后的同步（`save_credentials` 内部已做同样的热替换）。
    pub fn reload_credentials(&self, creds: Credentials) {
        if let Ok(mut guard) = self.credentials.write() {
            *guard = creds;
        }
    }

    /// 账号池快照（克隆）。
    pub fn pool(&self) -> crate::accounts::AccountPool {
        self.pool.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// 是否处于账号池模式（池中有账号）。
    pub fn pool_mode(&self) -> bool {
        self.pool.read().map(|g| g.is_pool_mode()).unwrap_or(false)
    }

    /// 取**指定账号**自己的凭据。
    ///
    /// 池模式下每个账号有独立的 token / cookie，必须按账号取——
    /// 凭据读取点（`client.rs` / `completion.rs` / `hif.rs`）全部走这里，
    /// 不再依赖「内存里刚好是这一组」的隐式时序。
    /// 池中找不到该 id（单账号模式 / 账号刚被移除）→ 回退到当前凭据。
    pub fn credentials_for(&self, account_id: &str) -> Credentials {
        if let Ok(guard) = self.pool.read() {
            if let Some(account) = guard.find(account_id) {
                return account.credentials();
            }
        }
        self.credentials()
    }

    pub fn credentials(&self) -> Credentials {
        self.credentials
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 当前是否已配置 token（对应 Python `Config.is_authenticated`）。
    pub fn is_authenticated(&self) -> bool {
        !self.credentials().user_token.is_empty()
    }
}

/// 以 0600 权限原子写入 JSON 文件（先写临时文件再 rename）。
pub fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().context("config path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("create dir {}", dir.display()))?;
    let json = serde_json::to_string_pretty(value)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod {}", tmp.display()))?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 序列化环境变量相关测试，避免并行测试互相污染。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn credentials_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("visionary-config-test-{}", std::process::id()));
        let path = dir.join("config.json");
        let creds = Credentials {
            user_token: "tok".into(),
            smid_v2: "smid".into(),
            cf_clearance: "cf".into(),
        };
        write_private_json(&path, &creds).expect("write should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).expect("metadata");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600, "must be 0600");
        }
        let read: Credentials = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read.user_token, "tok");
        assert_eq!(read.smid_v2, "smid");
        assert_eq!(read.cf_clearance, "cf");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_file_roundtrip_and_model_type_validation() {
        let dir =
            std::env::temp_dir().join(format!("visionary-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = SettingsFile {
            model_type: Some("ocr".into()),
        };
        write_private_json(&path, &settings).expect("write should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).expect("metadata");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600, "must be 0600");
        }
        let read: SettingsFile = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read.model_type.as_deref(), Some("ocr"));
        // 缺失字段容错
        let empty: SettingsFile = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.model_type, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_model_type_accepts_vision_and_ocr() {
        assert!(validate_model_type("vision").is_ok());
        assert!(validate_model_type("ocr").is_ok());
        let err = validate_model_type("empty").unwrap_err();
        assert!(err.to_string().contains("vision"), "unexpected: {err}");
        assert!(err.to_string().contains("ocr"), "unexpected: {err}");
    }

    #[test]
    fn data_dir_falls_back_to_userprofile_without_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_home = std::env::var_os("HOME");
        let old_userprofile = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", "C:\\Users\\testuser");

        // Windows 风格：无 HOME，仅 USERPROFILE → 回退到 USERPROFILE
        let dir = data_dir().expect("data_dir should fall back to USERPROFILE");
        assert_eq!(
            dir,
            PathBuf::from("C:\\Users\\testuser").join(".deepseek-visionary")
        );

        // 两者皆缺失 → 明确报错
        std::env::remove_var("USERPROFILE");
        let err = data_dir().unwrap_err();
        assert!(
            err.to_string().contains("HOME/USERPROFILE not set"),
            "unexpected error: {err}"
        );

        if let Some(old) = old_home {
            std::env::set_var("HOME", old);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(old) = old_userprofile {
            std::env::set_var("USERPROFILE", old);
        } else {
            std::env::remove_var("USERPROFILE");
        }
    }
}

//! 配置管理：环境变量 + `~/.deepseek-vision/config.json`。
//!
//! 对照 Python 版 `config.py`，并落实 design.md 决策 6：
//! - 键名对齐 Python：`user_token` / `smid_v2` / `cf_clearance`
//! - 环境变量覆盖：`DEEPSEEK_USER_TOKEN` / `DEEPSEEK_SMIDV2` / `DEEPSEEK_CF_CLEARANCE`
//! - 配置文件写入权限 0600
//! - `RwLock` 保护 + 热重载原子替换（login 工具写入后无需重启）

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

/// 数据目录：`~/.deepseek-vision/`，存放 config.json 与 session.json。
fn data_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME not set")?;
    Ok(home.join(".deepseek-vision"))
}

/// 配置文件路径：`~/.deepseek-vision/config.json`。
fn config_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("config.json"))
}

/// 会话文件路径：`~/.deepseek-vision/session.json`（对应 Python `_get_session_file`）。
pub fn session_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("session.json"))
}

/// 登录浏览器 profile 目录：`~/.deepseek-vision/browser/`（权限 0700）。
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
    pub base_url: String,
    pub poll_timeout: Duration,
    pub poll_interval: Duration,
    pub chat_timeout: Duration,
    pub upload_timeout: Duration,
    pub max_retries: u32,
    pub app_version: String,
    pub client_locale: String,
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
        Ok(Self {
            credentials: std::sync::Arc::new(RwLock::new(credentials)),
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

    /// 热重载：仅替换内存中的凭据（login 工具写入磁盘后调用）。
    /// 注：`save_credentials` 内部已做原子替换热重载，此方法仅保留供
    /// 外部已写入磁盘的场景手动同步（当前无调用方）。
    #[allow(dead_code)]
    pub fn reload_credentials(&self, creds: Credentials) {
        if let Ok(mut guard) = self.credentials.write() {
            *guard = creds;
        }
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

    /// 构造请求 cookies（对应 Python `Config.cookies`：仅 smidV2）。
    pub fn cookies(&self) -> Vec<(String, String)> {
        let c = self.credentials();
        let mut out = Vec::new();
        if !c.smid_v2.is_empty() {
            out.push(("smidV2".into(), c.smid_v2));
        }
        out
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

    #[test]
    fn credentials_roundtrip() {
        let dir = std::env::temp_dir().join(format!("dsv-config-test-{}", std::process::id()));
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
}

//! visionary-zed-ext — Zed 扩展壳（wasm32-wasip2）。
//!
//! 职责（对应 design.md 决策 1）：
//! - `context_server_command`：返回启动 `visionary-server` 原生二进制的命令。
//!   二进制来源优先级：`server_path` 设置 → 本地缓存 → GitHub Releases 下载。
//! - `context_server_configuration`：安装引导 + 可选的 `server_path` 设置。
//! - env 透传：`DEEPSEEK_USER_TOKEN` 从扩展进程环境读取并透传（覆盖 config.json，
//!   尽力而为）。注：zed_extension_api 0.7 的 `context_server_command` 只能拿到
//!   `Project`（仅 `worktree_ids()`），无法获取 `Worktree` 读取 `shell_env()`，
//!   因此无法按 worktree 环境透传（design 修订，见 design.md Open Questions）。
//!
//! 扩展本身在 WASM 沙箱内运行，不承载 vision 流水线；重活都在原生二进制里。

use schemars::JsonSchema;
use serde::Deserialize;
use std::env;
use zed::settings::ContextServerSettings;
use zed_extension_api::{
    self as zed, Command, ContextServerConfiguration, ContextServerId, DownloadedFileType,
    GithubReleaseOptions, Project, Result,
};

/// GitHub 仓库（发布 release 的二进制 asset 从这里下载）。
const GITHUB_REPO: &str = "xlight/deepseek-visionary";
/// 二进制文件名。
const SERVER_BINARY_NAME: &str = "visionary-server";
/// 扩展 ID（与 extension.toml 的 context_servers 段名一致）。
const CONTEXT_SERVER_ID: &str = "deepseek-visionary";

/// 可选的用户设置：`server_path` 指向本地二进制（开发/调试）。
#[derive(Debug, Deserialize, JsonSchema)]
struct VisionarySettings {
    /// 本地 visionary-server 二进制路径（可选；不设置时自动下载）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server_path: Option<String>,
}

struct VisionaryExtension;

impl zed::Extension for VisionaryExtension {
    fn new() -> Self {
        Self
    }

    fn context_server_command(
        &mut self,
        _context_server_id: &ContextServerId,
        project: &Project,
    ) -> Result<Command> {
        // 1. 读取用户设置
        let settings = ContextServerSettings::for_project(CONTEXT_SERVER_ID, project)?;
        let settings: Option<VisionarySettings> = settings
            .settings
            .map(|v| zed::serde_json::from_value(v).map_err(|e| e.to_string()))
            .transpose()?;

        // 2. 确定二进制路径（优先级：server_path → 本地缓存 → 下载）
        let server_path = match settings.as_ref().and_then(|s| s.server_path.clone()) {
            Some(path) => path,
            None => ensure_server_binary()?,
        };

        // 3. 构建 Command：透传 DEEPSEEK_USER_TOKEN（若扩展进程环境中有）
        let mut env = vec![];
        if let Ok(value) = env::var("DEEPSEEK_USER_TOKEN") {
            if !value.is_empty() {
                env.push(("DEEPSEEK_USER_TOKEN".into(), value));
            }
        }

        Ok(Command {
            command: server_path,
            args: vec![],
            env,
        })
    }

    fn context_server_configuration(
        &mut self,
        _context_server_id: &ContextServerId,
        _project: &Project,
    ) -> Result<Option<ContextServerConfiguration>> {
        let installation_instructions =
            include_str!("../configuration/installation_instructions.md").to_string();
        let default_settings = include_str!("../configuration/default_settings.jsonc").to_string();
        let settings_schema = zed::serde_json::to_string(&schemars::schema_for!(VisionarySettings))
            .map_err(|e| e.to_string())?;

        Ok(Some(ContextServerConfiguration {
            installation_instructions,
            default_settings,
            settings_schema,
        }))
    }
}

/// 获取本地缓存或下载二进制，返回可执行路径。
///
/// 缓存位置：扩展目录下 `.visionary-server/<version>/visionary-server`。
/// 版本对比：与 GitHub 最新 release 版本比较，不一致则重新下载。
fn ensure_server_binary() -> Result<String> {
    let base_dir = env::current_dir().map_err(|e| e.to_string())?;
    let cache_root = base_dir.join(".visionary-server");

    // 查最新 release
    let release = zed::latest_github_release(
        GITHUB_REPO,
        GithubReleaseOptions {
            require_assets: true,
            pre_release: false,
        },
    )?;
    let version = &release.version;
    let version_dir = cache_root.join(version);
    let binary_path = version_dir.join(SERVER_BINARY_NAME);

    if !binary_path.exists() {
        // 需要下载：选与当前平台匹配的 asset
        let (os, arch) = zed::current_platform();
        let asset_name = asset_name_for_platform(SERVER_BINARY_NAME, os, arch);
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                format!("release {version} has no asset `{asset_name}` for {os:?}/{arch:?}")
            })?;

        zed::download_file(
            &asset.download_url,
            &binary_path.to_string_lossy(),
            DownloadedFileType::Uncompressed,
        )?;
        zed::make_file_executable(&binary_path.to_string_lossy())?;
    }

    Ok(binary_path.to_string_lossy().to_string())
}

/// 计算当前平台的 asset 文件名（如 `visionary-server-aarch64-apple-darwin`）。
fn asset_name_for_platform(binary: &str, os: zed::Os, arch: zed::Architecture) -> String {
    let os_str = match os {
        zed::Os::Mac => "apple-darwin",
        zed::Os::Linux => "unknown-linux-gnu",
        zed::Os::Windows => "pc-windows-msvc",
    };
    let arch_str = match arch {
        zed::Architecture::Aarch64 => "aarch64",
        zed::Architecture::X8664 => "x86_64",
        zed::Architecture::X86 => "i686",
    };
    let ext = if os == zed::Os::Windows { ".exe" } else { "" };
    format!("{binary}-{arch_str}-{os_str}{ext}")
}

zed::register_extension!(VisionaryExtension);

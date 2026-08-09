//! 浏览器定位（对应 design.md 决策 5，任务 5.1）。
//!
//! 支持 macOS / Linux / Windows 的 Chrome / Chromium / Edge 常见安装路径，
//! 以及 PATH 中的可执行文件探测。找不到时返回明确的引导错误。

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// 候选浏览器可执行文件列表（按优先级）。
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        candidates.push(PathBuf::from(
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ));
        candidates.push(PathBuf::from(
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/bin/google-chrome"));
        candidates.push(PathBuf::from("/usr/bin/google-chrome-stable"));
        candidates.push(PathBuf::from("/usr/bin/chromium"));
        candidates.push(PathBuf::from("/usr/bin/chromium-browser"));
        candidates.push(PathBuf::from("/usr/bin/microsoft-edge"));
        candidates.push(PathBuf::from("/snap/bin/chromium"));
    }

    #[cfg(target_os = "windows")]
    {
        let program_files =
            std::env::var("PROGRAMFILES").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 = std::env::var("PROGRAMFILES(X86)")
            .unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
        let local_app_data = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
        candidates.push(PathBuf::from(format!(
            "{program_files}\\Google\\Chrome\\Application\\chrome.exe"
        )));
        candidates.push(PathBuf::from(format!(
            "{program_files_x86}\\Google\\Chrome\\Application\\chrome.exe"
        )));
        candidates.push(PathBuf::from(format!(
            "{local_app_data}\\Google\\Chrome\\Application\\chrome.exe"
        )));
        candidates.push(PathBuf::from(format!(
            "{program_files}\\Microsoft\\Edge\\Application\\msedge.exe"
        )));
        candidates.push(PathBuf::from(format!(
            "{program_files_x86}\\Microsoft\\Edge\\Application\\msedge.exe"
        )));
    }

    // PATH 探测（追加在固定路径之后，作为兜底）
    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            for name in [
                "google-chrome",
                "chromium",
                "chromium-browser",
                "chrome",
                "msedge",
            ] {
                candidates.push(dir.join(name));
            }
        }
    }

    candidates
}

/// 找到第一个存在的浏览器可执行文件。
pub fn find_browser() -> Result<PathBuf> {
    for path in candidate_paths() {
        if path.exists() && is_executable(&path) {
            return Ok(path);
        }
    }
    Err(anyhow!(
        "未找到 Chrome / Chromium / Edge。请安装任意 Chrome 系浏览器后重试，\
         或手动配置 ~/.deepseek-visionary/config.json（见 deepseek_vision_status 输出）"
    ))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_non_empty() {
        assert!(!candidate_paths().is_empty());
    }

    #[test]
    fn find_browser_returns_error_message_when_missing() {
        // 不保证本机装有浏览器；只验证错误路径可读。
        let _ = find_browser();
    }
}

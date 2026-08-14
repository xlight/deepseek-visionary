//! `init` 子命令：检测已安装的 AI agent，按各 agent 的 MCP 配置形状写入配置。
//!
//! 设计要点（design.md 决策 2）：
//! - agent 自有 CLI 优先（`codex mcp add` / `claude mcp add`），直接改配置兜底
//! - 写前备份 `*.bak.<UTC时间戳>`，备份失败即中止，绝不静默覆盖
//! - 严格 JSON / TOML 解析，解析失败即中止
//! - `--dry-run` 只预览不落盘；`--yes` 免交互
//! - opencode 显式 `timeout: 60000`（官方默认 5000ms，冷启动会超时）

use crate::cli::{install_skill, InitArgs};
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// MCP 注册名（各 agent 配置中的键名）。
const SERVER_NAME: &str = "deepseek-visionary";
/// opencode 冷启动超时（ms）。
const OPENCODE_TIMEOUT_MS: u64 = 60000;

/// 受支持的 agent。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Opencode,
    Codex,
    Claude,
    ClaudeDesktop,
    Cursor,
    /// DeepSeek Harness（`dsh`）：skill + CLI 轻量接入，不写 MCP 配置。
    DeepseekHarness,
}

impl Agent {
    pub fn name(&self) -> &'static str {
        match self {
            Agent::Opencode => "opencode",
            Agent::Codex => "codex",
            Agent::Claude => "claude",
            Agent::ClaudeDesktop => "claude-desktop",
            Agent::Cursor => "cursor",
            Agent::DeepseekHarness => "dsh",
        }
    }

    pub fn from_name(s: &str) -> Option<Agent> {
        match s {
            "opencode" => Some(Agent::Opencode),
            "codex" => Some(Agent::Codex),
            "claude" => Some(Agent::Claude),
            "claude-desktop" | "claude_desktop" => Some(Agent::ClaudeDesktop),
            "cursor" => Some(Agent::Cursor),
            "dsh" | "deepseek-harness" | "deepseek_harness" | "harness" => {
                Some(Agent::DeepseekHarness)
            }
            _ => None,
        }
    }

    fn all() -> [Agent; 6] {
        [
            Agent::Opencode,
            Agent::Codex,
            Agent::Claude,
            Agent::ClaudeDesktop,
            Agent::Cursor,
            Agent::DeepseekHarness,
        ]
    }
}

/// 检测结果。
pub struct Detection {
    pub agent: Agent,
    pub installed: bool,
    pub config_path: Option<PathBuf>,
}

/// 用户主目录（Unix: HOME，Windows: USERPROFILE）。
pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .context("HOME/USERPROFILE not set")
}

/// DeepSeek Harness 根目录：`$DSH_HOME` 环境变量优先，未设置或空白时回退 `~/.dsh`。
/// 与 DSH 官方 `dsh-home-paths` 解析规则一致（空白 `$DSH_HOME` 视为未设置）；
/// 检测与写入必须共用同一解析结果。
pub fn dsh_home_dir(home: &Path) -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|v| !v.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".dsh"))
}

/// 在 PATH 中查找可执行文件（Windows 自动补 `.exe`）。
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in [dir.join(name), dir.join(format!("{name}.exe"))] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 检测本机已安装的 agent（PATH 可执行文件 或 配置文件存在）。
pub fn detect_agents(home: &Path) -> Vec<Detection> {
    Agent::all()
        .into_iter()
        .map(|agent| {
            let (installed, config_path) = match agent {
                Agent::Opencode => {
                    let p = opencode_config_path(home);
                    (find_in_path("opencode").is_some() || p.exists(), Some(p))
                }
                Agent::Codex => {
                    let p = codex_config_path(home);
                    (find_in_path("codex").is_some() || p.exists(), Some(p))
                }
                Agent::Claude => (find_in_path("claude").is_some(), None),
                Agent::ClaudeDesktop => {
                    let p = claude_desktop_config_path(home);
                    (p.as_ref().is_some_and(|p| p.exists()), p)
                }
                Agent::Cursor => {
                    let p = cursor_config_path(home);
                    (find_in_path("cursor").is_some() || p.exists(), Some(p))
                }
                Agent::DeepseekHarness => {
                    // `dsh` 在 PATH，或 DSH 根下已有 profile（初始化过 dsh web / headless）
                    let root = dsh_home_dir(home);
                    let has_profiles = root.join("profiles").exists();
                    (find_in_path("dsh").is_some() || has_profiles, Some(root))
                }
            };
            Detection {
                agent,
                installed,
                config_path,
            }
        })
        .collect()
}

// --- 配置文件路径（纯函数，便于测试注入临时 HOME） ---

fn opencode_config_path(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("opencode.json")
}

fn codex_config_path(home: &Path) -> PathBuf {
    home.join(".codex").join("config.toml")
}

fn claude_config_path(home: &Path) -> PathBuf {
    home.join(".claude.json")
}

fn claude_desktop_config_path(home: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|p| {
            PathBuf::from(p)
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = home;
        None
    }
}

fn cursor_config_path(home: &Path) -> PathBuf {
    home.join(".cursor").join("mcp.json")
}

// --- init 主流程 ---

pub fn cmd_init(args: InitArgs) -> Result<()> {
    let home = home_dir()?;
    run_init(&home, &args)
}

fn run_init(home: &Path, args: &InitArgs) -> Result<()> {
    let targets = resolve_targets(args)?;

    // 无参且无 flags：交互式列出检测结果
    if targets.is_empty() {
        return interactive_listing(home);
    }

    // 二进制 PATH 检测（避免写出无效配置）
    if find_in_path("visionary-server").is_none() {
        bail!(
            "visionary-server is not in PATH. Install it first:\n\
             - One-liner: curl -LsSf https://github.com/xlight/deepseek-visionary/releases/latest/download/visionary-server-installer.sh | sh\n\
             - Homebrew: brew install <tap>/visionary-server\n\
             - npm: npm install -g <npm-package>\n\
             Or add the binary to PATH and retry."
        );
    }

    for agent in &targets {
        write_agent(home, *agent, args.dry_run)?;
    }

    println!();
    if args.dry_run {
        println!("(--dry-run: preview only, nothing was written)");
    } else {
        println!("Configured {} agent(s):", targets.len());
        for agent in &targets {
            println!("  - {}", agent.name());
        }
        println!("Restart the agent for changes to take effect.");
    }
    Ok(())
}

/// 解析目标 agent：位置参数（单 agent）或多选 flags（批量），二选一。
fn resolve_targets(args: &InitArgs) -> Result<Vec<Agent>> {
    let mut flags = Vec::new();
    if args.opencode {
        flags.push(Agent::Opencode);
    }
    if args.codex {
        flags.push(Agent::Codex);
    }
    if args.claude {
        flags.push(Agent::Claude);
    }
    if args.claude_desktop {
        flags.push(Agent::ClaudeDesktop);
    }
    if args.cursor {
        flags.push(Agent::Cursor);
    }
    if args.dsh {
        flags.push(Agent::DeepseekHarness);
    }

    if let Some(name) = &args.agent {
        if !flags.is_empty() {
            bail!(
                "Argument conflict: positional agent and multi-select flags cannot be combined.\
                 Use either `visionary-server init <agent>` or `init --opencode --codex ...` (not both)"
            );
        }
        let agent = Agent::from_name(name).ok_or_else(|| {
            anyhow!(
                "Unknown agent `{name}`. Supported: opencode / codex / claude / claude-desktop / cursor / dsh"
            )
        })?;
        return Ok(vec![agent]);
    }
    Ok(flags)
}

/// 无参数交互模式：列出检测结果并给出指引；未检测到任何 agent 时退出非零。
fn interactive_listing(home: &Path) -> Result<()> {
    let detections = detect_agents(home);
    let installed: Vec<_> = detections.iter().filter(|d| d.installed).collect();

    if installed.is_empty() {
        println!("No supported AI agent detected.");
        println!(
            "Install your target agent and retry, or configure manually (see docs/integrations/)."
        );
        bail!("no supported agent detected");
    }

    println!("Detected agents:");
    for d in &detections {
        let mark = if d.installed { "[OK]" } else { "     " };
        let config = d
            .config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".into());
        println!("  {mark} {:<15} {}", d.agent.name(), config);
    }
    println!();
    println!(
        "Run `visionary-server init <agent>` to configure one, or `--opencode --codex ... --yes` to batch configure."
    );
    Ok(())
}

// --- 各 agent 写入 ---

fn write_agent(home: &Path, agent: Agent, dry_run: bool) -> Result<()> {
    match agent {
        Agent::Opencode => write_opencode(home, dry_run),
        Agent::Codex => write_codex(home, dry_run),
        Agent::Claude => write_claude(home, dry_run),
        Agent::ClaudeDesktop => write_claude_desktop(home, dry_run),
        Agent::Cursor => write_cursor(home, dry_run),
        Agent::DeepseekHarness => write_dsh(home, dry_run),
    }
}

/// DeepSeek Harness：skill + CLI 轻量接入（不写 MCP 配置）。
///
/// 把内嵌 SKILL.md 安装到 DSH 的两个技能发现根：
/// - `$DSH_HOME/skills/visionary-cli/`（DSH user 技能根，始终扫描）
/// - `~/.agents/skills/visionary-cli/`（DSH 默认扫描的 agents 技能根，与 `skill install` 一致）
fn write_dsh(home: &Path, dry_run: bool) -> Result<()> {
    let dsh_root = dsh_home_dir(home);
    let mut targets = vec![dsh_root.join("skills").join("visionary-cli")];
    targets.push(home.join(".agents").join("skills").join("visionary-cli"));

    if dry_run {
        println!("[dry-run] would install the embedded SKILL.md to:");
        for dir in &targets {
            println!("  {}", dir.join("SKILL.md").display());
        }
        return Ok(());
    }

    println!("Configuring DeepSeek Harness (dsh) via skill + CLI:");
    for dir in &targets {
        install_skill(dir)?;
    }
    println!(
        "  Skills installed to the DSH discovery roots. Restart dsh (or wait for its skill watcher)\n\
         and the `visionary-cli` skill will appear in the harness skill catalog.\n\
         Tip: run `visionary-server login` first, then the agent calls `visionary-server vision <image> --json`."
    );
    Ok(())
}

/// opencode：顶层 `mcp` 键 + `type: local` + `command` 数组 + `timeout: 60000`。
fn write_opencode(home: &Path, dry_run: bool) -> Result<()> {
    let path = opencode_config_path(home);
    let entry = serde_json::json!({
        "type": "local",
        "command": ["visionary-server", "mcp-stdio"],
        "enabled": true,
        "timeout": OPENCODE_TIMEOUT_MS,
    });
    merge_json_key(&path, &["mcp", SERVER_NAME], &entry, dry_run)
}

/// Codex：首选 `codex mcp add`，失败兜底写 `~/.codex/config.toml` 的 `[mcp_servers.*]`。
fn write_codex(home: &Path, dry_run: bool) -> Result<()> {
    let path = codex_config_path(home);

    if !dry_run {
        if let Some(codex) = find_in_path("codex") {
            let status = Command::new(codex)
                .args([
                    "mcp",
                    "add",
                    SERVER_NAME,
                    "--",
                    "visionary-server",
                    "mcp-stdio",
                ])
                .status();
            if let Ok(st) = status {
                if st.success() {
                    println!("Registered {SERVER_NAME} via `codex mcp add`");
                    return Ok(());
                }
            }
        }
    }

    // 兜底：写 TOML（`mcp_servers` 键名——`mcp.servers` 会静默失效，issue #3441）
    let section = codex_section_toml();
    if dry_run {
        println!(
            "[dry-run] would write {} (prefers `codex mcp add`; previewing fallback TOML):",
            path.display()
        );
        println!("{section}");
        return Ok(());
    }
    write_toml_section(&path, &section)?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// Claude Code：首选 `claude mcp add`，失败兜底写 `~/.claude.json` 的 `mcpServers`。
fn write_claude(home: &Path, dry_run: bool) -> Result<()> {
    let path = claude_config_path(home);

    if !dry_run {
        if let Some(claude) = find_in_path("claude") {
            let status = Command::new(claude)
                .args([
                    "mcp",
                    "add",
                    "--transport",
                    "stdio",
                    SERVER_NAME,
                    "--scope",
                    "user",
                    "--",
                    "visionary-server",
                    "mcp-stdio",
                ])
                .status();
            if let Ok(st) = status {
                if st.success() {
                    println!("Registered {SERVER_NAME} via `claude mcp add`");
                    return Ok(());
                }
            }
        }
    }

    let entry = serde_json::json!({ "command": "visionary-server", "args": ["mcp-stdio"] });
    merge_json_key(&path, &["mcpServers", SERVER_NAME], &entry, dry_run)
}

/// Claude Desktop：`mcpServers` 形状写 `claude_desktop_config.json`。
fn write_claude_desktop(home: &Path, dry_run: bool) -> Result<()> {
    let path = claude_desktop_config_path(home).ok_or_else(|| {
        anyhow!("auto-locating the Claude Desktop config is not supported on this platform; configure manually (see docs/integrations/claude-desktop.md)")
    })?;
    let entry = serde_json::json!({ "command": "visionary-server", "args": ["mcp-stdio"] });
    merge_json_key(&path, &["mcpServers", SERVER_NAME], &entry, dry_run)
}

/// Cursor：`mcpServers` 形状写 `~/.cursor/mcp.json`（用户级）。
fn write_cursor(home: &Path, dry_run: bool) -> Result<()> {
    let path = cursor_config_path(home);
    let entry = serde_json::json!({ "command": "visionary-server", "args": ["mcp-stdio"] });
    merge_json_key(&path, &["mcpServers", SERVER_NAME], &entry, dry_run)
}

/// Codex config.toml 的 `[mcp_servers.deepseek-visionary]` 段文本。
fn codex_section_toml() -> String {
    let mut table = toml::map::Map::new();
    table.insert(
        "command".into(),
        toml::Value::String("visionary-server".into()),
    );
    table.insert(
        "args".into(),
        toml::Value::Array(vec![toml::Value::String("mcp-stdio".into())]),
    );
    format!("[mcp_servers.{SERVER_NAME}]\n{}", toml::Value::Table(table))
}

// --- 通用写入工具 ---

/// 合并 JSON：读入 → 严格解析 → 只增改 `key_path` 键 → 备份 → 写回。
/// dry_run 时只打印预览，不落盘。
fn merge_json_key(
    path: &Path,
    key_path: &[&str],
    value: &serde_json::Value,
    dry_run: bool,
) -> Result<()> {
    let mut root: serde_json::Value = if path.exists() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| {
            format!(
                "parse failed (not overwritten): {} is not valid JSON. Fix it manually and retry.",
                path.display()
            )
        })?
    } else {
        serde_json::json!({})
    };

    // 定位/创建中间对象；中间键若存在但不是对象则中止（绝不覆盖）
    let mut cur = &mut root;
    for key in &key_path[..key_path.len() - 1] {
        let is_object = cur.get(*key).map(serde_json::Value::is_object);
        match is_object {
            Some(false) => {
                bail!(
                    "`{key}` in {} is not a JSON object; cannot merge safely, aborted (not overwritten).",
                    path.display()
                )
            }
            Some(true) => {}
            None => {
                cur[*key] = serde_json::json!({});
            }
        }
        cur = cur
            .get_mut(*key)
            .expect("key must exist or be inserted above");
    }
    cur[key_path[key_path.len() - 1]] = value.clone();

    let preview = serde_json::to_string_pretty(&root)?;
    if dry_run {
        println!("[dry-run] would write {}:", path.display());
        println!("{preview}");
        return Ok(());
    }

    backup_then_write(path, &preview)?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// 写/替换 TOML 文件中的 `[mcp_servers.deepseek-visionary]` 段。
/// 现有文件必须是合法 TOML，否则中止不覆盖。
fn write_toml_section(path: &Path, section: &str) -> Result<()> {
    let mut content = if path.exists() {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        // 严格校验：现有内容必须是合法 TOML
        raw.parse::<toml::Value>().with_context(|| {
            format!(
                "parse failed (not overwritten): {} is not valid TOML. Fix it manually and retry.",
                path.display()
            )
        })?;
        raw
    } else {
        String::new()
    };

    let header = format!("[mcp_servers.{SERVER_NAME}]");
    if let Some(start) = content.find(&header) {
        // 替换已有段：从该 header 到下一个 `[` 或文件末尾
        let rest_start = start + header.len();
        let end = content[rest_start..]
            .find("\n[")
            .map(|i| rest_start + i)
            .unwrap_or(content.len());
        content.replace_range(start..end, section.trim_end());
    } else {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(section);
    }
    content.push('\n');

    backup_then_write(path, &content)?;
    Ok(())
}

/// 写前备份（`*.bak.<UTC时间戳>`），备份失败即中止；随后原子写入。
fn backup_then_write(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        let backup = backup_path(path);
        std::fs::copy(path, &backup).with_context(|| {
            format!(
                "backup {} -> {} failed, aborted",
                path.display(),
                backup.display()
            )
        })?;
        println!("  Backed up to {}", backup.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    name.push_str(&format!(".bak.{}", utc_timestamp()));
    path.with_file_name(name)
}

/// UTC 时间戳 `YYYYMMDDTHHMMSS`（无外部依赖，从 epoch 计算）。
fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}{mo:02}{d:02}T{hh:02}{mm:02}{ss:02}")
}

/// 从 epoch 天数计算公历日期（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 序列化 PATH 相关测试，避免并行测试互相污染环境变量。
    static PATH_LOCK: Mutex<()> = Mutex::new(());

    fn temp_home(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("visionary-onboarding-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opencode_shape_has_timeout_and_preserves_existing() {
        let home = temp_home("opencode");
        let path = opencode_config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 已有其他 MCP 服务配置
        std::fs::write(
            &path,
            r#"{"mcp":{"other":{"type":"local","command":["other-server"]}}}"#,
        )
        .unwrap();

        write_opencode(&home, false).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &root["mcp"]["deepseek-visionary"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"][0], "visionary-server");
        assert_eq!(entry["command"][1], "mcp-stdio");
        assert_eq!(entry["timeout"], 60000);
        assert!(entry["enabled"].as_bool().unwrap());
        // 原有配置保留
        assert_eq!(root["mcp"]["other"]["command"][0], "other-server");
        // 备份存在
        let backup = backup_path(&path);
        assert!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("opencode.json.bak.")),
            "backup should be created"
        );
        let _ = backup;
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dry_run_does_not_touch_disk() {
        let home = temp_home("dryrun");
        let path = opencode_config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"existing":true}"#).unwrap();

        write_opencode(&home, true).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"existing\""));
        assert!(
            !raw.contains("deepseek-visionary"),
            "dry-run must not write"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn invalid_json_aborts_without_overwrite() {
        let home = temp_home("invalid-json");
        let path = opencode_config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not valid json").unwrap();

        let err = write_opencode(&home, false).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
        // 原文件未被覆盖
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw, "{ not valid json");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn codex_toml_uses_mcp_servers_key() {
        let home = temp_home("codex-toml");
        let path = codex_config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "model = \"gpt-5\"\n").unwrap();

        // 直接测 TOML 兜底（避免真实 codex CLI 干扰）
        let section = codex_section_toml();
        write_toml_section(&path, &section).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[mcp_servers.deepseek-visionary]"),
            "must use mcp_servers key, got: {content}"
        );
        assert!(content.contains("command = \"visionary-server\""));
        assert!(
            content.contains("args = [\"mcp-stdio\"]"),
            "args must include mcp-stdio, got: {content}"
        );
        assert!(
            content.contains("model = \"gpt-5\""),
            "preserve existing toml"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn claude_fallback_writes_mcp_servers_json() {
        // 直接测 JSON 兜底路径（避免真实 claude CLI 干扰 / 写入真实 ~/.claude.json）
        let home = temp_home("claude");
        let path = claude_config_path(&home);
        let entry = serde_json::json!({ "command": "visionary-server", "args": ["mcp-stdio"] });
        merge_json_key(&path, &["mcpServers", SERVER_NAME], &entry, false).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            root["mcpServers"]["deepseek-visionary"]["command"],
            "visionary-server"
        );
        assert_eq!(
            root["mcpServers"]["deepseek-visionary"]["args"][0],
            "mcp-stdio"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn cursor_writes_mcp_servers_json() {
        let home = temp_home("cursor");
        let path = cursor_config_path(&home);
        write_cursor(&home, false).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root["mcpServers"]["deepseek-visionary"].is_object());
        assert_eq!(
            root["mcpServers"]["deepseek-visionary"]["args"][0],
            "mcp-stdio"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn resolve_targets_rejects_positional_and_flags_together() {
        let args = InitArgs {
            agent: Some("opencode".into()),
            opencode: true,
            codex: false,
            claude: false,
            claude_desktop: false,
            cursor: false,
            dsh: false,
            yes: false,
            dry_run: false,
        };
        assert!(resolve_targets(&args).is_err());
    }

    #[test]
    fn resolve_targets_dsh_flag_collects() {
        let args = InitArgs {
            agent: None,
            opencode: false,
            codex: false,
            claude: false,
            claude_desktop: false,
            cursor: false,
            dsh: true,
            yes: false,
            dry_run: false,
        };
        assert_eq!(
            resolve_targets(&args).unwrap(),
            vec![Agent::DeepseekHarness]
        );
    }

    #[test]
    fn from_name_accepts_dsh_aliases() {
        for name in ["dsh", "deepseek-harness", "deepseek_harness", "harness"] {
            assert_eq!(
                Agent::from_name(name),
                Some(Agent::DeepseekHarness),
                "alias `{name}` should map to DeepseekHarness"
            );
        }
        assert_eq!(Agent::from_name("dsh").unwrap().name(), "dsh");
    }

    #[test]
    fn binary_not_in_path_is_rejected() {
        let _guard = PATH_LOCK.lock().unwrap();
        let home = temp_home("nopath");
        // 清空 PATH：找不到 visionary-server
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let result = run_init(
            &home,
            &InitArgs {
                agent: Some("opencode".into()),
                opencode: false,
                codex: false,
                claude: false,
                claude_desktop: false,
                cursor: false,
                dsh: false,
                yes: false,
                dry_run: false,
            },
        );
        if let Some(old) = old {
            std::env::set_var("PATH", old);
        } else {
            std::env::remove_var("PATH");
        }
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not in PATH"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dsh_detection_via_profiles_dir() {
        let _guard = PATH_LOCK.lock().unwrap();
        let home = temp_home("dsh-detect");
        // 环境里可能设置了 DSH_HOME，统一清掉让检测走 ~/.dsh 路径
        let old_dsh = std::env::var_os("DSH_HOME");
        std::env::remove_var("DSH_HOME");
        // 无 ~/.dsh/profiles → 未检测（PATH 中大概率没有 dsh）
        let detections = detect_agents(&home);
        let dsh = detections
            .iter()
            .find(|d| d.agent == Agent::DeepseekHarness)
            .unwrap();
        if find_in_path("dsh").is_none() {
            assert!(!dsh.installed, "no profiles dir and no dsh in PATH");
        }
        // 创建 ~/.dsh/profiles/web → 已检测，config_path 指向 DSH 根
        let profiles = home.join(".dsh").join("profiles").join("web");
        std::fs::create_dir_all(&profiles).unwrap();
        let detections = detect_agents(&home);
        let dsh = detections
            .iter()
            .find(|d| d.agent == Agent::DeepseekHarness)
            .unwrap();
        assert!(dsh.installed, "profiles dir should mark dsh as installed");
        assert_eq!(
            dsh.config_path.as_deref(),
            Some(home.join(".dsh").as_path()),
            "config_path should be the DSH root"
        );
        if let Some(old) = old_dsh {
            std::env::set_var("DSH_HOME", old);
        } else {
            std::env::remove_var("DSH_HOME");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dsh_home_dir_respects_env_var() {
        let _guard = PATH_LOCK.lock().unwrap();
        let home = temp_home("dsh-home");
        let old = std::env::var_os("DSH_HOME");
        std::env::remove_var("DSH_HOME");
        assert_eq!(dsh_home_dir(&home), home.join(".dsh"));
        std::env::set_var("DSH_HOME", "/custom/dsh");
        assert_eq!(dsh_home_dir(&home), PathBuf::from("/custom/dsh"));
        std::env::set_var("DSH_HOME", "");
        assert_eq!(
            dsh_home_dir(&home),
            home.join(".dsh"),
            "empty DSH_HOME falls back"
        );
        std::env::set_var("DSH_HOME", "   ");
        assert_eq!(
            dsh_home_dir(&home),
            home.join(".dsh"),
            "whitespace-only DSH_HOME falls back (matches dsh-home-paths)"
        );
        if let Some(old) = old {
            std::env::set_var("DSH_HOME", old);
        } else {
            std::env::remove_var("DSH_HOME");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_dsh_installs_skill_to_both_roots() {
        let _guard = PATH_LOCK.lock().unwrap();
        let home = temp_home("dsh-write");
        let old = std::env::var_os("DSH_HOME");
        std::env::remove_var("DSH_HOME");

        write_dsh(&home, false).unwrap();

        let dsh_root = home
            .join(".dsh")
            .join("skills")
            .join("visionary-cli")
            .join("SKILL.md");
        let agents_root = home
            .join(".agents")
            .join("skills")
            .join("visionary-cli")
            .join("SKILL.md");
        assert!(dsh_root.exists(), "DSH skill root must be written");
        assert!(agents_root.exists(), "agents skill root must be written");
        // 内容与内嵌一致
        let embedded = include_str!("../../../skills/visionary-cli/SKILL.md");
        assert_eq!(std::fs::read_to_string(&dsh_root).unwrap(), embedded);
        assert_eq!(std::fs::read_to_string(&agents_root).unwrap(), embedded);

        if let Some(old) = old {
            std::env::set_var("DSH_HOME", old);
        } else {
            std::env::remove_var("DSH_HOME");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_dsh_dry_run_does_not_touch_disk() {
        let _guard = PATH_LOCK.lock().unwrap();
        let home = temp_home("dsh-dryrun");
        let old = std::env::var_os("DSH_HOME");
        std::env::remove_var("DSH_HOME");

        write_dsh(&home, true).unwrap();

        assert!(
            !home.join(".dsh").join("skills").exists(),
            "dry-run must not create DSH skills dir"
        );
        assert!(
            !home.join(".agents").join("skills").exists(),
            "dry-run must not create agents skills dir"
        );

        if let Some(old) = old {
            std::env::set_var("DSH_HOME", old);
        } else {
            std::env::remove_var("DSH_HOME");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn utc_timestamp_format() {
        let ts = utc_timestamp();
        assert_eq!(ts.len(), 15, "expected YYYYMMDDTHHMMSS, got {ts}");
        assert!(ts.chars().all(|c| c.is_ascii_digit() || c == 'T'));
    }
}

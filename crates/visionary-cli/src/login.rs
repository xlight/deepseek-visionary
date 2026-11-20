//! 浏览器自动登录（对应 design.md 决策 5，任务 5.2/5.3/5.4/5.5）。
//!
//! 流程：定位 Chrome 系浏览器 → 以专用 profile（`~/.deepseek-visionary/browser/`，
//! 0700）+ `--remote-debugging-port` 启动（Chrome 136+ 要求非默认 profile 才允许
//! 远程调试，天然满足）→ 打开 chat.deepseek.com → 轮询等待用户登录 →
//! 抓取 `localStorage.userToken` 与 `smidV2` / `cf_clearance` cookie →
//! 写入 `~/.deepseek-visionary/config.json`（0600）并热重载 → 返回。
//!
//! 反自动化检测：chromiumoxide 默认追加 `--enable-automation`（`navigator.webdriver=true`），
//! DeepSeek 登录页的 hCaptcha 会据此判定为机器人、人机验证永远无法通过。因此启动时追加
//! `--disable-blink-features=AutomationControlled`，并在导航前先建空白页调用
//! `enable_stealth_mode_with_agent` 注入隐藏 webdriver / permissions / plugins / WebGL
//! 指纹的初始化脚本（对目标页及 hCaptcha iframe 均生效），再跳转 chat.deepseek.com。
//!
//! 登录页语言：默认跟随系统 locale（`LC_ALL` / `LANG`，自适应），可用
//! `DEEPSEEK_LOGIN_LANG` 环境变量手动切换（如 `zh-CN` / `en`）。chromiumoxide
//! 默认 `--lang=en_US` 会把页面固定为英文版，故按解析结果追加 `--lang` / `--accept-lang`。
//!
//! `run_login` 阻塞等待登录完成（带超时）；超时后浏览器保持打开，用户可继续
//! 登录后重跑 login（幂等）。手动粘贴 token 为兜底路径。

use crate::account_health;
use crate::browser;
use crate::client::{ApiClient, MutedFailure};
use crate::config::{self, Config, Credentials};
use anyhow::{anyhow, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// 登录等待超时（可用 `DEEPSEEK_LOGIN_TIMEOUT` 覆盖，秒）。
fn login_timeout() -> Duration {
    let secs = std::env::var("DEEPSEEK_LOGIN_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    Duration::from_secs(secs)
}

/// 自动续期（token 失效后自动刷新）的登录等待超时（可用 `DEEPSEEK_REFRESH_TIMEOUT` 覆盖，秒）。
///
/// 比手动登录短：续期走的是**已登录的持久 profile**，页面加载后几秒即可拿到新 token，
/// 不需要用户交互；若超时，多半是人机验证挡住了，此时快速失败比干等 10 分钟好。
pub(crate) fn refresh_timeout() -> Duration {
    let secs = std::env::var("DEEPSEEK_REFRESH_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(90);
    Duration::from_secs(secs)
}

/// 登录模式：交互登录 vs 自动续期。
///
/// 用**显式枚举**区分，SHALL NOT 依赖超时时长数值——`login_timeout()` 与
/// `refresh_timeout()` 都是环境变量可配，取值可能相同或被调换，靠数值区分不可靠。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMode {
    /// 交互登录：遇静音保持浏览器打开，等待用户换号。
    Interactive,
    /// 自动续期：遇静音快速失败并关窗，SHALL NOT 等用户换号。
    AutoRefresh,
}

/// 探针结果 → 登录状态机的下一步动作（纯函数，便于单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginDecision {
    /// 账号健康：返回凭据（调用方随后关窗并保存）。
    Healthy,
    /// 账号被静音（交互模式）：提示换号 + 注入横幅 + **继续等待**。
    Muted {
        /// 该账号是否**首次**提示（同一 `account_id` 只提示一次）。
        first_notice: bool,
    },
    /// 账号被静音（自动续期模式）：快速失败（调用方随后关窗）。
    MutedFatal,
    /// 探测失败：降级为「未知健康」，继续等待。
    Unknown,
}

/// 依据探测结果、模式与「是否已提示过」判定下一步动作。
///
/// 换号判定走**两段式**：token 变化只作为「候选变化」信号，
/// 真正的换号以探针返回的 `account_id` 为准——同一账号刷新同样会轮换 token。
pub fn decide_next_step(
    health: Option<&crate::account_health::AccountHealth>,
    mode: LoginMode,
    already_hinted: bool,
) -> LoginDecision {
    match health {
        None => LoginDecision::Unknown,
        Some(h) if h.muted => {
            if mode == LoginMode::AutoRefresh {
                LoginDecision::MutedFatal
            } else {
                LoginDecision::Muted {
                    first_notice: !already_hinted,
                }
            }
        }
        Some(_) => LoginDecision::Healthy,
    }
}

/// 记录已提示过的静音账号；返回 `true` 表示该账号**首次**提示。
pub fn mark_muted_notice(hinted: &mut HashSet<String>, account_id: &str) -> bool {
    hinted.insert(account_id.to_string())
}

/// 登录超时错误（携带最近一次探测到的静音截止时间）。
pub fn login_timeout_error(timeout: Duration, last_mute_until: Option<u64>) -> anyhow::Error {
    let extra = match last_mute_until {
        Some(ts) => format!(
            "; the muted account stays muted until {}",
            crate::client::format_epoch_utc(ts as i64)
        ),
        None => String::new(),
    };
    anyhow!(
        "Login timed out after {} seconds{}",
        timeout.as_secs(),
        extra
    )
}
/// 登录过程中的中间事件（供 MCP 通道把「换号指引」即时返回给调用方）。
#[derive(Debug, Clone)]
pub enum LoginEvent {
    /// 当前登录的账号被静音。
    Muted {
        account_id: String,
        label: String,
        mute_until: Option<u64>,
    },
}

/// 换号指引文案（CLI stderr 与 MCP 返回文本**共用同一份**，保证两个通道语义一致）。
pub fn muted_guidance(label: &str, account_id: &str, mute_until: Option<u64>) -> String {
    let who = if label.trim().is_empty() {
        account_id
    } else {
        label.trim()
    };
    let until = match mute_until {
        Some(ts) => format!(
            "{} (unix {})",
            crate::client::format_epoch_utc(ts as i64),
            ts
        ),
        None => "unknown (the server did not report a deadline)".to_string(),
    };
    format!(
        "The account you signed in to is MUTED (upload risk control).\n\
         - Account: {who} (id: {account_id})\n\
         - Muted until: {until}\n\
         The browser window is still open. Please log out of this account in that window and sign in with another one;\n\
         the login continues automatically once a usable account is detected."
    )
}

/// 页面横幅文案（HTML 片段，供 `innerHTML` 注入，醒目地引导换号登录）。
///
/// 保留账号标识与静音截止时间（UTC 文本），便于用户确认是哪个号、何时解封；
/// 用 `<br>` 分行、加粗强调动作，比纯 `textContent` 的单行提示更易读。
fn muted_banner_html(label: &str, account_id: &str, mute_until: Option<u64>) -> String {
    let who = if label.trim().is_empty() {
        account_id
    } else {
        label.trim()
    };
    let until = match mute_until {
        Some(ts) => format!(
            "{} (unix {})",
            crate::client::format_epoch_utc(ts as i64),
            ts
        ),
        None => "未知截止时间".to_string(),
    };
    format!(
        "<strong>⚠ 当前账号已被静音，请换号登录</strong><br>\
         账号 {who} 触发上传风控，静音截止：{until}。<br>\
         <strong>请在本窗口先登出，再用另一个账号登录</strong>；检测到可用账号后会自动继续。\
         <br><span style=\"opacity:.85;font-size:13px\">（本提示不遮挡页面操作，可点右上角 × 关闭）</span>"
    )
}
/// 浏览器页面里的静音提示横幅节点 id。
const BANNER_ID: &str = "__deepseek_visionary_muted_banner__";

/// 横幅正文的子节点 id（关闭按钮是**兄弟节点**，刷新正文时不能被一起抹掉）。
const BANNER_BODY_ID: &str = "__deepseek_visionary_muted_banner_body__";

/// 用户「关闭提示」的持久标记（sessionStorage，同源同会话有效）。
///
/// 横幅每轮轮询都会重新注入（对抗 SPA 导航/重载清空），若不记住用户的关闭操作，
/// 横幅会一次次弹回来。记下后同一会话内不再自动弹出，用户仍可从 CLI stderr 看到指引。
const BANNER_DISMISS_KEY: &str = "__deepseek_visionary_muted_banner_dismissed__";

/// 构造注入横幅的 JS 脚本。
///
/// 关键设计（避免横幅挡住页面操作）：
/// - 容器 `pointer-events: none` —— **绝不拦截**页面上的点击/输入，
///   横幅覆盖到的按钮依然可点（这是本条需求的硬约束）；
/// - 只有右上角的「×」按钮 `pointer-events: auto`，供用户手动收起；
/// - 定位在**左下角浮层**而非顶部通栏，避免遮住页面顶部的导航/模型选择等按钮；
/// - 正文放在独立子节点里，刷新正文不会把「×」按钮一起抹掉。
fn muted_banner_script(html: &str) -> String {
    let id = serde_json::to_string(BANNER_ID).unwrap_or_else(|_| "\"\"".to_string());
    let body_id = serde_json::to_string(BANNER_BODY_ID).unwrap_or_else(|_| "\"\"".to_string());
    let body = serde_json::to_string(html).unwrap_or_else(|_| "\"\"".to_string());
    let dismiss = serde_json::to_string(BANNER_DISMISS_KEY).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(() => {{
  const id = {id};
  const bodyId = {body_id};
  const html = {body};
  const dismissKey = {dismiss};

  // 用户已手动关闭 → 本会话内不再自动弹回。
  let dismissed = false;
  try {{ dismissed = sessionStorage.getItem(dismissKey) === '1'; }} catch (e) {{ dismissed = false; }}
  if (dismissed) {{
    const old = document.getElementById(id);
    if (old) old.remove();
    return false;
  }}

  let el = document.getElementById(id);
  if (!el) {{
    el = document.createElement('div');
    el.id = id;
    el.setAttribute('role', 'status');
    el.setAttribute('aria-live', 'polite');
    // pointer-events:none → 横幅永不拦截页面交互（覆盖到的按钮照样能点）。
    el.style.cssText = 'position:fixed;left:16px;bottom:16px;z-index:2147483647;max-width:min(420px,calc(100vw - 32px));padding:12px 38px 12px 14px;background:#b42318;color:#fff;font:14px/1.6 system-ui,-apple-system,sans-serif;text-align:left;box-shadow:0 6px 20px rgba(0,0,0,.35);border-radius:10px;white-space:normal;pointer-events:none;';

    const body = document.createElement('div');
    body.id = bodyId;
    body.style.cssText = 'pointer-events:none;';
    el.appendChild(body);

    const close = document.createElement('button');
    close.type = 'button';
    close.setAttribute('aria-label', '关闭静音提示');
    close.textContent = '\u00d7';
    // 只有这个按钮可以点，其余区域一律穿透到页面。
    close.style.cssText = 'position:absolute;top:4px;right:6px;width:26px;height:26px;padding:0;border:0;border-radius:6px;background:rgba(255,255,255,.18);color:#fff;font:18px/20px system-ui,sans-serif;cursor:pointer;pointer-events:auto;';
    close.addEventListener('click', (ev) => {{
      ev.preventDefault();
      ev.stopPropagation();
      try {{ sessionStorage.setItem(dismissKey, '1'); }} catch (e) {{}}
      const n = document.getElementById(id);
      if (n) n.remove();
    }});
    el.appendChild(close);

    (document.body || document.documentElement).appendChild(el);
  }}

  const body = document.getElementById(bodyId);
  if (body) body.innerHTML = html;
  return true;
}})()"#
    )
}

/// 注入静音提示横幅。
///
/// 页面是 SPA：换号引发的导航/重载会清空注入的节点，因此**每次探测到静音都重新注入**，
/// 不依赖「一次注入长期存活」。注入失败 SHALL NOT 中断流程（CLI stderr 提示仍生效）。
async fn inject_muted_banner(browser: &Browser, html: &str) {
    // attach 模式下用户盯着的是自己手动打开的标签页，不是程序新建的登录 tab；
    // 因此横幅 SHALL 注入到浏览器里**所有**页面，确保用户一定能看到换号提示。
    let pages = match browser.pages().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("failed to enumerate browser pages for the muted banner: {e}");
            return;
        }
    };
    tracing::info!(
        "injecting the muted-account banner into {} browser page(s)",
        pages.len()
    );
    let js = muted_banner_script(html);
    for page in &pages {
        if let Err(e) = page.evaluate(js.as_str()).await {
            // 某些页面（chrome:// 内部页）不允许 evaluate，跳过即可，不影响其它页面。
            tracing::debug!("skipped muted banner on one page: {e}");
        }
    }
}

/// 移除静音提示横幅（探测到健康后）——同样作用于所有页面。
///
/// 同时清掉「已关闭」标记：等下一轮又遇到静音账号时，横幅应当重新出现。
async fn remove_muted_banner(browser: &Browser) {
    let id = serde_json::to_string(BANNER_ID).unwrap_or_else(|_| "\"\"".to_string());
    let dismiss = serde_json::to_string(BANNER_DISMISS_KEY).unwrap_or_else(|_| "\"\"".to_string());
    let js = format!(
        r#"(() => {{
  try {{ sessionStorage.removeItem({dismiss}); }} catch (e) {{}}
  const el = document.getElementById({id});
  if (el) el.remove();
  return true;
}})()"#
    );
    if let Ok(pages) = browser.pages().await {
        for page in &pages {
            let _ = page.evaluate(js.as_str()).await;
        }
    }
}
/// 静音态下的**重探间隔**：账号被静音时不必每秒打一次探针。
///
/// 横幅仍会在每轮轮询时重新注入（页面可能被换号导航/重载清空），
/// 只是不再每秒重复请求 API。换号会改变 token，**立即**触发新探测，不受该间隔影响。
const MUTED_REPROBE_INTERVAL: Duration = Duration::from_secs(5);
/// 轮询间隔：1 秒。
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 登录页语言解析（自适应 + 手动切换）。
///
/// 优先级：
/// 1. `DEEPSEEK_LOGIN_LANG` 环境变量显式指定（如 `zh-CN` / `en` / `ja`）——手动切换；
/// 2. 系统 `LC_ALL` / `LC_MESSAGES` / `LANG`（如 `zh_CN.UTF-8` → `zh-CN`）——自适应；
/// 3. 均缺失时默认 `zh-CN`（DeepSeek 为中文产品，目标用户以中文为主）。
///
/// 页面语言由浏览器 `Accept-Language` 决定（chromiumoxide 默认 `--lang=en_US`
/// 会把登录页固定成英文版），因此按此结果追加 `--lang` / `--accept-lang` 启动参数。
fn login_lang() -> String {
    if let Ok(v) = std::env::var("DEEPSEEK_LOGIN_LANG") {
        let v = v.trim();
        if !v.is_empty() {
            return normalize_lang(v);
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if let Some(l) = derive_lang_from_locale(&v) {
                return l;
            }
        }
    }
    "zh-CN".to_string()
}

/// 归一化用户显式指定的语言标签（`zh-cn` / `zh_CN` → `zh-CN`）。
fn normalize_lang(lang: &str) -> String {
    let parts: Vec<&str> = lang.trim().split(['-', '_']).collect();
    match parts.as_slice() {
        [l] => l.to_ascii_lowercase(),
        [l, r] => format!("{}-{}", l.to_ascii_lowercase(), r.to_ascii_uppercase()),
        _ => parts[0].to_ascii_lowercase(),
    }
}

/// 从系统 locale（`zh_CN.UTF-8` / `en_US` / `C`）推导语言标签；无法识别返回 None。
fn derive_lang_from_locale(locale: &str) -> Option<String> {
    let loc = locale.trim();
    if loc.is_empty() || loc == "C" || loc == "POSIX" {
        return None;
    }
    let lang_part = loc.split('.').next().unwrap_or(loc);
    Some(normalize_lang(lang_part))
}

/// 构造 `--accept-lang` 值：主语言优先，英文兜底。
fn accept_lang(lang: &str) -> String {
    let base = lang.split('-').next().unwrap_or(lang).to_ascii_lowercase();
    match base.as_str() {
        "zh" => "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
        "en" => "en-US,en;q=0.9".to_string(),
        "ja" => "ja-JP,ja;q=0.9,en;q=0.8".to_string(),
        _ => format!("{lang},{base};q=0.9,en;q=0.8"),
    }
}

/// `deepseek_vision_login` 工具实现（任务 5.4）与 CLI `login` 子命令共用。
///
/// 返回人类可读文本（MCP handler 包装成 `CallToolResult`，CLI 直接打印）。
pub async fn run_login(config: &Config) -> Result<String> {
    run_login_with_mode(config, LoginMode::Interactive, None).await
}

/// 与 [`run_login`] 相同，但把登录过程中的中间事件（静音）回传给出调用方。
///
/// MCP 通道用它把「换号指引」即时返回给调用方，同时让浏览器流程继续在后台等待换号——
/// 换号成功后由后台流程自动保存凭据。
pub async fn run_login_with_events(
    config: &Config,
    events: tokio::sync::mpsc::UnboundedSender<LoginEvent>,
) -> Result<String> {
    run_login_with_mode(config, LoginMode::Interactive, Some(events)).await
}

/// `accounts add` 专用入口：**绕过**「已配置 token」的幂等检查。
///
/// 既有 `run_login` 在已配置 token 时直接返回「Token already configured」、不打开浏览器——
/// 那会让「已有账号时添加第二个号」永远开不了浏览器。
///
/// 返回可区分结论：
/// - 新增账号 → 报告已登记；
/// - 该账号已在池中（共享浏览器 profile 带着上一个账号的登录态）→ 明确报告
///   「本次未新增账号」并提示先在浏览器里登出当前账号，SHALL NOT 报成添加成功。
pub async fn run_login_add_account(config: &Config) -> Result<String> {
    let mut events = None;
    add_account_flow(config, events.as_mut()).await
}

/// 与 [`run_login_add_account`] 相同，但回传登录过程中的中间事件（静音换号指引）。
pub async fn run_login_add_account_with_events(
    config: &Config,
    events: tokio::sync::mpsc::UnboundedSender<LoginEvent>,
) -> Result<String> {
    let mut events = Some(events);
    add_account_flow(config, events.as_mut()).await
}

/// 「添加账号」登录流程主体（绕过幂等检查 + 登记写回）。
async fn add_account_flow(
    config: &Config,
    events: Option<&mut tokio::sync::mpsc::UnboundedSender<LoginEvent>>,
) -> Result<String> {
    let (creds, health) = login_flow(config, login_timeout(), LoginMode::Interactive, events)
        .await
        .context("browser login flow failed")?;
    let outcome = persist_and_register(config, &creds, &health)?;
    match outcome {
        crate::accounts::RegisterOutcome::Added => Ok(format!(
            "Account added to the pool: {} (id: {})\n\
             The pool now rotates across all registered accounts.",
            display_label(&health),
            health.account_id
        )),
        crate::accounts::RegisterOutcome::Updated => Ok(format!(
            "This account is ALREADY in the pool — no new account was added: {} (id: {})\n\
             The shared browser profile still carries the previous login session.\n\
             To add a different account, log out of this account in the browser window first,\n\
             then run `visionary-cli accounts add` again.",
            display_label(&health),
            health.account_id
        )),
    }
}

/// 登录流程主体（阻塞等待）。
async fn run_login_with_mode(
    config: &Config,
    mode: LoginMode,
    events: Option<tokio::sync::mpsc::UnboundedSender<LoginEvent>>,
) -> Result<String> {
    // 已配置 token 时幂等提示。
    // 池模式例外：池模式下 `login` 用于**添加 / 更新**账号（登记进池），
    // 不能因为兼容配置里已有凭据就拒绝打开浏览器。
    if config.is_authenticated() && !config.pool_mode() {
        return Ok(
            "Token already configured. To re-login, run `visionary-cli logout` first.".into(),
        );
    }

    let mut events = events;
    match login_flow(config, login_timeout(), mode, events.as_mut()).await {
        Ok((creds, health)) => {
            // 登记写回：先写 accounts.json（池为权威）再写 config.json（兼容写回）。
            let outcome = persist_and_register(config, &creds, &health)?;
            let registered = match outcome {
                crate::accounts::RegisterOutcome::Added => {
                    format!(
                        "- Account registered in the pool: {}",
                        display_label(&health)
                    )
                }
                crate::accounts::RegisterOutcome::Updated => {
                    format!(
                        "- Account already in the pool (credentials updated): {}\n\
                         If you wanted to add a different account, log out of it in the browser window first, then run `visionary-cli login` again.",
                        display_label(&health)
                    )
                }
            };
            Ok(format!(
                "Login successful! Credentials saved:\n\
                 - user_token: {}...\n\
                 - smidV2: {}\n\
                 - cf_clearance: {}\n\
                 You can now analyze images with `visionary-cli vision`.\n\
                 {}",
                registered,
                mask(&creds.user_token),
                if creds.smid_v2.is_empty() {
                    "none".to_string()
                } else {
                    mask(&creds.smid_v2)
                },
                if creds.cf_clearance.is_empty() {
                    "none".to_string()
                } else {
                    mask(&creds.cf_clearance)
                },
            ))
        }
        Err(e) => {
            // 返回 Err：CLI 据此退出非零；MCP handler 映射回 CallToolResult::error。
            // 错误正文（启动失败 / 换号指引 / 超时）由 e 自带；这里只补通用兜底路径。
            let detail = format!("{e:#}");
            Err(anyhow!(
                "{}\nIf a browser window is open, finish logging in there and rerun\n\
                 `visionary-cli login`; or configure ~/.deepseek-visionary/config.json manually.",
                detail
            ))
        }
    }
}

/// `deepseek_vision_logout` 工具实现（任务 5.5）与 CLI `logout` 子命令共用。
pub async fn run_logout(config: &Config) -> Result<String> {
    config
        .save_credentials(&Credentials::default())
        .context("failed to clear credentials")?;
    if config.pool_mode() {
        // 池模式：logout 只清空兼容用的单账号配置，**不**静默移除池中账号。
        return Ok("Saved credentials cleared.\n\
             The account pool (~/.deepseek-visionary/accounts.json) is **unchanged** — its accounts are still used for rotation.\n\
             To drop a specific account, run `visionary-cli accounts remove <id>`."
            .into());
    }
    Ok("Saved credentials cleared.\n\
         To log in again, run `visionary-cli login`."
        .into())
}

/// 刷新凭据（token 自动续期的底层实现）。
///
/// 与 `run_login` 的区别：不检查"是否已配置 token"，直接重跑浏览器登录流程并覆盖保存。
/// DeepSeek 网页端每次加载页面都会轮换 `localStorage.userToken` 并作废旧 token，
/// 而 CLI 存的是快照——token 失效时只能靠这条路径重新拿一个。
pub async fn refresh_credentials(config: &Config, timeout: Duration) -> Result<Credentials> {
    // 自动续期走 **AutoRefresh** 模式：遇静音立即失败并关窗，SHALL NOT 等用户换号
    // （无人值守的浏览器干等满 600s 预算毫无意义）。
    let (creds, health) = login_flow(config, timeout, LoginMode::AutoRefresh, None)
        .await
        .context("browser login flow failed while refreshing credentials")?;
    // 续期同样走登记写回（凭据更新 + 账号状态刷新），SHALL NOT 只写 config.json。
    persist_and_register(config, &creds, &health)?;
    Ok(creds)
}

/// 登记写回：**先写 accounts.json（池为权威）再写 config.json（兼容写回）**。
///
/// - **顺序**：先池后兼容配置。两个文件不是原子写，池是权威来源、兼容配置是派生结果。
/// - **失败语义**：任一写失败都如实报错（SHALL NOT 报成完整成功）；
///   池写成功而兼容配置写失败时，下次登记会修正兼容配置（以池为准）。
/// - **返回**：区分「新增账号」与「该账号已在池中（仅更新凭据）」。
///
/// 登录 / 自动续期 / `accounts add` 三条路径 SHALL 全部走它，
/// SHALL NOT 各自散写 `save_credentials` 后再补登记。
pub fn persist_and_register(
    config: &Config,
    creds: &Credentials,
    health: &crate::account_health::AccountHealth,
) -> Result<crate::accounts::RegisterOutcome> {
    // 兼容配置写回（先做，保证即便池写失败凭据也不会丢）。
    // 池是权威：成功后再写兼容配置；两者不一致时以池为准，下次登记自愈。
    let account = crate::accounts::PoolAccount {
        account_id: health.account_id.clone(),
        label: health.label.clone(),
        user_token: creds.user_token.clone(),
        smid_v2: creds.smid_v2.clone(),
        cf_clearance: creds.cf_clearance.clone(),
        status: if health.muted {
            crate::accounts::AccountStatus::Muted
        } else {
            crate::accounts::AccountStatus::Healthy
        },
        mute_until: health.mute_until,
        last_used: None,
    };

    let outcome = if account.account_id.is_empty() {
        // 探针没给出稳定标识：SHALL NOT 写入占位标识（占位 id 与日后真实 id 不一致，
        // 会让同一账号出现两条、后续移除删不掉）。此时只更新兼容配置。
        tracing::warn!("account health probe returned no account id; skipping pool registration");
        None
    } else {
        let path = crate::accounts::pool_file()?;
        let mut outcome = crate::accounts::RegisterOutcome::Added;
        crate::accounts::merge_and_save(&path, |pool| {
            outcome = pool.register(account.clone());
            Ok(())
        })?;
        Some(outcome)
    };

    // 兼容写回：config.json（保持既有引用不破）。
    config
        .save_credentials(creds)
        .context("write compatibility config.json")?;

    // 同步内存池（长驻进程后续轮询立即可见新账号）。
    if let Ok(mut pool) = config.pool.write() {
        if !account.account_id.is_empty() {
            pool.register(account);
        }
    }

    Ok(outcome.unwrap_or(crate::accounts::RegisterOutcome::Updated))
}

/// 账号展示标签（缺省退回账号 id）。
fn display_label(health: &crate::account_health::AccountHealth) -> String {
    if health.label.is_empty() {
        health.account_id.clone()
    } else {
        health.label.clone()
    }
}

/// 完整登录流程：启动浏览器 → 等待登录 → 抓取凭据。
async fn login_flow(
    config: &Config,
    timeout: Duration,
    mode: LoginMode,
    events: Option<&mut tokio::sync::mpsc::UnboundedSender<LoginEvent>>,
) -> Result<(Credentials, crate::account_health::AccountHealth)> {
    let browser_path = browser::find_browser()?;
    let profile_dir = config::browser_profile_dir()?;

    // 专用 profile 目录（0700）
    std::fs::create_dir_all(&profile_dir).context("failed to create browser profile dir")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o700));
    }

    let port = pick_free_port().context("no free port for remote debugging")?;

    // 登录页语言：默认跟随系统 locale（自适应），可用 DEEPSEEK_LOGIN_LANG 手动切换。
    // 不传 --accept-lang 时 chromiumoxide 默认 --lang=en_US 会把页面固定成英文版。
    let lang = login_lang();
    let accept_lang = accept_lang(&lang);

    let browser_config = BrowserConfig::builder()
        .chrome_executable(&browser_path)
        .with_head()
        .user_data_dir(&profile_dir)
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 隐藏 automation 指纹：chromiumoxide 默认带 --enable-automation，
        // 登录页 hCaptcha 会据此拒绝通过人机验证（配合下方 stealth 脚本双保险）
        .arg("--disable-blink-features=AutomationControlled")
        // 登录页语言（zh-CN / en / …）：--accept-lang 决定页面渲染语言，
        // --lang 覆盖 chromiumoxide 的默认 en_US UI locale
        .arg(format!("--lang={lang}"))
        .arg(format!("--accept-lang={accept_lang}"))
        .build()
        .map_err(|e| anyhow!("failed to build browser config: {e}"))?;

    tracing::info!(
        "launching browser {} with profile {} on port {port} (lang {lang}, accept-lang {accept_lang})",
        browser_path.display(),
        profile_dir.display()
    );

    // 优先按 v0.7.8 原样用 chromiumoxide 直接拉起。
    // 启动失败时按**真实原因**分流：
    // - 进程完整性级别低于 Medium（沙箱给工作区构建产物打了 Low 标签）
    //   → Chrome 一定起不来，直接给出准确修复指令，不再让用户手动开 Chrome；
    // - 其它失败 → 仍然允许接管用户已启动的带调试端口的 Chrome。
    let (mut browser, mut handler, attached) = match Browser::launch(browser_config).await {
        Ok((browser, handler)) => (browser, handler, false),
        Err(e) => {
            if crate::integrity::running_below_medium() {
                let level = crate::integrity::current_level()
                    .map(|l| l.name())
                    .unwrap_or("Low");
                let detail = format!("{e:#}");
                return Err(anyhow!(
                    concat!(
                        "failed to launch browser: {detail}\n",
                        "Cause: this process runs at {level} integrity, so the browser it starts\n",
                        "inherits the same low integrity level and aborts immediately\n",
                        "(exit code 0x80000003 / STATUS_BREAKPOINT, empty stderr).\n",
                        "Fix: restore the Medium integrity label on this executable, then retry:\n",
                        "  icacls \"{exe}\" /setintegritylevel M\n",
                        "See docs/cli.md (Chrome cannot be launched) for details."
                    ),
                    detail = detail,
                    level = level,
                    exe = browser_path.display(),
                ));
            }
            tracing::warn!(
                "direct browser launch failed ({e:#}); trying to attach to an existing Chrome"
            );
            let ws = attach_ws_url(port).await.ok_or_else(|| {
                let detail = format!("{e:#}");
                anyhow!(
                    concat!(
                        "{}\n",
                        "Start Chrome yourself with the same profile, then rerun `visionary-cli login`:\n",
                        "  PowerShell:  & \"{exe}\" --remote-debugging-port={port} --user-data-dir=\"{profile}\"\n",
                        "  cmd / bash: \"{exe}\" --remote-debugging-port={port} --user-data-dir=\"{profile}\"\n",
                        "The login command will attach to that running Chrome and finish."
                    ),
                    detail,
                    port = port,
                    exe = browser_path.display(),
                    profile = profile_dir.display(),
                )
            })?;
            tracing::info!("attached to existing Chrome at {ws}");
            let (browser, handler) = Browser::connect(ws)
                .await
                .context("failed to attach to browser")?;
            (browser, handler, true)
        }
    };

    // handler drain loop（chromiumoxide 必需）
    let handler_task = tokio::spawn(async move { while let Some(_h) = handler.next().await {} });

    let result = wait_for_login(&mut browser, &profile_dir, timeout, mode, config, events).await;

    // 清理：直接启动的浏览器由 chromiumoxide 关闭；attach 的是用户自己的窗口，
    // SHALL NOT 关闭。专用 profile 中的登录会话始终保留，重跑 login 可续接。
    handler_task.abort();
    if !attached {
        let _ = browser.close().await;
    }
    result
}

/// 读取本机 DevTools 端点的 WebSocket URL。
///
/// 先看本次分配的端口，再看默认调试端口 9222（用户手动启动 Chrome 时的习惯端口）。
/// 用于 `Browser::launch` 失败后接管用户已启动的调试实例。
async fn attach_ws_url(port: u16) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    for candidate in [port, 9222u16] {
        let url = format!("http://127.0.0.1:{candidate}/json/version");
        let ws = match client.get(&url).send().await {
            Ok(resp) => resp.json::<serde_json::Value>().await.ok().and_then(|v| {
                v.get("webSocketDebuggerUrl")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            }),
            Err(_) => None,
        };
        if let Some(ws) = ws {
            return Some(ws);
        }
    }
    None
}
/// 等待用户登录、抓取凭据并**探测账号健康**。
///
/// 循环内的分支：
/// - 健康 → 移除横幅、返回凭据（调用方随后关窗并保存）；
/// - 被静音 → 提示换号 + 注入横幅，**不 return**，继续等用户换号；
/// - 自动续期模式遇静音 → 立即返回静音错误（快速失败，不等换号）。
///
/// 探测**必须**用候选凭据（本次抓到的 token）发起：候选凭据此时尚未写入配置，
/// 沿用当前凭据会问错账号（首次登录则因凭据为空而误报鉴权失败）。
#[allow(clippy::too_many_arguments)]
async fn wait_for_login(
    browser: &mut Browser,
    _profile_dir: &PathBuf,
    timeout: Duration,
    mode: LoginMode,
    config: &Config,
    mut events: Option<&mut tokio::sync::mpsc::UnboundedSender<LoginEvent>>,
) -> Result<(Credentials, crate::account_health::AccountHealth)> {
    // 先创建空白页并注入反自动化脚本（隐藏 navigator.webdriver 等指纹），
    // 再导航到 chat.deepseek.com —— 否则登录页的 hCaptcha 检测到自动化
    // 浏览器后，人机验证始终无法通过（"过不了 capture"）。
    let page = browser
        .new_page("about:blank")
        .await
        .context("failed to create browser page")?;

    // 读取真实 UA 并原样保留：伪装成别的系统反而更容易被风控识别
    let real_ua = page
        .evaluate("navigator.userAgent")
        .await
        .ok()
        .and_then(|v| v.value().and_then(|x| x.as_str()).map(String::from))
        .unwrap_or_default();

    page.enable_stealth_mode_with_agent(&real_ua)
        .await
        .context("failed to enable anti-detection stealth mode")?;

    page.goto("https://chat.deepseek.com")
        .await
        .context("failed to open chat.deepseek.com")?;
    let deadline = tokio::time::Instant::now() + timeout;
    // 探针超时 SHALL 短于该模式的总预算（交互 600s / 自动续期 90s，两者不同）。
    let probe_budget = account_health::probe_timeout(timeout);
    let mut captcha_hinted = false;
    // 同一 account_id 的静音提示只输出一次；换新账号后允许再次提示。
    let mut hinted_accounts: HashSet<String> = HashSet::new();
    // 最近一次探测到的静音截止时间（供超时错误携带）。
    let mut last_mute_until: Option<u64> = None;
    // 已完成判定（健康）的 token。探针因**网络原因**失败时不更新它，
    // 以便下一轮重试同一 token；token 失效（AuthFailure）时更新，避免无效重试。
    let mut settled_token = String::new();
    // 上一次探测结论（时间戳 + 健康快照）：用于静音态的探测节流与横幅重注入。
    let mut last_probe: Option<(tokio::time::Instant, crate::account_health::AccountHealth)> = None;

    loop {
        if tokio::time::Instant::now() >= deadline {
            // 超时 → 报错（调用方随后关窗）。错误携带静音截止时间，便于判断何时可再试。
            return Err(login_timeout_error(timeout, last_mute_until));
        }
        // 检测到人机验证（hCaptcha）时提示一次，引导用户在浏览器窗口手动完成
        if !captcha_hinted {
            let has_captcha = page
                .evaluate("document.querySelector('iframe[src*=\"hcaptcha\"]') !== null")
                .await
                .ok()
                .and_then(|v| v.value().and_then(|x| x.as_bool()))
                .unwrap_or(false);
            if has_captcha {
                tracing::info!(
                    "detected hCaptcha on the DeepSeek sign-in page; \
                     please solve it manually in the browser window"
                );
                captcha_hinted = true;
            }
        }

        // 读取 localStorage.userToken（页面 localStorage 里存的是 JSON 字符串，
        // 形如 {"value": "...", ...}；未登录时为 {"value":null,...}）
        let token_json: Option<String> = page
            .evaluate("localStorage.getItem('userToken')")
            .await
            .ok()
            .and_then(|v| v.value().and_then(|x| x.as_str()).map(String::from));

        if let Some(json) = token_json {
            let token = parse_token_json(&json)?;
            if !token.is_empty() {
                // 探测时机：token 变化（换号候选）**立即**探测；
                // 同一 token 且上次结论为静音时，按 `MUTED_REPROBE_INTERVAL` 节流重探。
                let token_changed = token != settled_token;
                let reprobe_due = last_probe
                    .as_ref()
                    .map(|(at, _)| at.elapsed() >= MUTED_REPROBE_INTERVAL)
                    .unwrap_or(false);
                if token_changed || reprobe_due {
                    // 已登录成功，抓取 cookies
                    let cookies = page.get_cookies().await.unwrap_or_default();
                    let smid_v2 = find_cookie(&cookies, "smidV2").unwrap_or_default();
                    let cf_clearance = find_cookie(&cookies, "cf_clearance").unwrap_or_default();
                    let creds = Credentials {
                        user_token: token.clone(),
                        smid_v2,
                        cf_clearance,
                    };

                    // 用**候选凭据**构造独立配置与探针 client：
                    // `with_credentials` 会换一个全新的 Arc<RwLock<Credentials>>，
                    // 不会污染配置中当前生效的凭据（SHALL NOT 用 config.clone()）。
                    let candidate_config = config.with_credentials(creds.clone());
                    let client = match ApiClient::new(candidate_config) {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("failed to build the account health probe client: {e}");
                            ApiClient::new(config.clone()).context("failed to build api client")?
                        }
                    };

                    let probe_result =
                        account_health::probe_account_with_timeout(&client, probe_budget).await;
                    let health = match &probe_result {
                        Ok(h) => Some(h),
                        Err(e) => {
                            // 探针失败 → 降级为「未知健康」，继续等待，SHALL NOT 中断等待循环。
                            if e.downcast_ref::<crate::client::AuthFailure>().is_some() {
                                // token 失效：不再重探同一 token，等页面给出新 token。
                                settled_token = token.clone();
                            }
                            tracing::warn!(
                            "account health probe failed ({e}); continuing to wait for a usable account"
                        );
                            None
                        }
                    };
                    let already_hinted = health
                        .map(|h| hinted_accounts.contains(&h.account_id))
                        .unwrap_or(false);

                    match decide_next_step(health, mode, already_hinted) {
                        LoginDecision::Healthy => {
                            // 健康：移除横幅并返回凭据 + 账号档案（调用方随后关窗并登记）。
                            remove_muted_banner(browser).await;
                            let h = health.expect("healthy decision implies a health payload");
                            return Ok((creds, h.clone()));
                        }
                        LoginDecision::MutedFatal => {
                            // 自动续期模式：快速失败（调用方随后关窗），SHALL NOT 等用户换号。
                            let h = health.expect("muted decision implies a health payload");
                            return Err(MutedFailure::new(
                                format!(
                                    "account {} ({}) is muted",
                                    if h.label.is_empty() {
                                        &h.account_id
                                    } else {
                                        &h.label
                                    },
                                    h.account_id
                                ),
                                h.mute_until,
                            )
                            .into());
                        }
                        LoginDecision::Muted { first_notice } => {
                            // 账号被静音：保持浏览器打开，提示换号，继续等待。
                            let h = health.expect("muted decision implies a health payload");
                            last_mute_until = h.mute_until;
                            // 记录本次结论：同一 token 不再算「变化」，
                            // 后续按 `MUTED_REPROBE_INTERVAL` 节流重探。
                            settled_token = token.clone();
                            last_probe = Some((tokio::time::Instant::now(), h.clone()));

                            // 提示去重：同一 account_id 只提示一次。
                            if first_notice
                                && mark_muted_notice(&mut hinted_accounts, &h.account_id)
                            {
                                let guidance =
                                    muted_guidance(&h.label, &h.account_id, h.mute_until);
                                // CLI：写 stderr（stdout 留给结构化输出）。
                                crate::out::eprint_display(guidance);
                                // MCP：把指引回传给调用方，由它即时返回。
                                if let Some(tx) = events.as_deref_mut() {
                                    let _ = tx.send(LoginEvent::Muted {
                                        account_id: h.account_id.clone(),
                                        label: h.label.clone(),
                                        mute_until: h.mute_until,
                                    });
                                }
                            }

                            // 横幅：每次探测到静音都重新注入（页面是 SPA，
                            // 换号引发的导航/重载会清空注入的节点）。
                            inject_muted_banner(
                                browser,
                                &muted_banner_html(&h.label, &h.account_id, h.mute_until),
                            )
                            .await;

                            // **不 return**：继续轮询，等待用户在同一窗口换号。
                            // 同一 token 的重探已按 `MUTED_REPROBE_INTERVAL` 节流；
                            // 横幅则每轮轮询都会重新注入（页面可能被重载清空）。
                        }
                        LoginDecision::Unknown => {
                            // 探测失败 → 继续等待（上面已记 warning）。
                        }
                    }
                } else if let Some((_, health)) = &last_probe {
                    // 复用上次结论（账号仍静音）：只重新注入横幅，不重复打 API。
                    if health.muted {
                        inject_muted_banner(
                            browser,
                            &muted_banner_html(
                                &health.label,
                                &health.account_id,
                                health.mute_until,
                            ),
                        )
                        .await;
                    }
                }
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
/// 解析 localStorage userToken JSON（`JSON.parse(value).value`）。
///
/// 兼容四种形态：
/// - 未登录：`null` / `""` / `{"value":null}` → 返回空字符串（继续轮询等待）
/// - 已登录：`{"value": "..."}`（网页版当前格式）
/// - 兜底：纯字符串 token
fn parse_token_json(json: &str) -> Result<String> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(String::new());
    }
    let v: serde_json::Value =
        serde_json::from_str(trimmed).context("userToken is not valid JSON")?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(map) => match map.get("value") {
            // 未登录时 DeepSeek 存的是 {"value":null,"__version":"0"}
            Some(serde_json::Value::Null) | None => Ok(String::new()),
            Some(serde_json::Value::String(s)) if !s.is_empty() => Ok(s.clone()),
            _ => Err(anyhow!("userToken has unexpected shape: {trimmed}")),
        },
        _ => Err(anyhow!("userToken has unexpected shape: {trimmed}")),
    }
}

/// 从 CDP cookies 中按名称取值。
fn find_cookie(
    cookies: &[chromiumoxide::cdp::browser_protocol::network::Cookie],
    name: &str,
) -> Option<String> {
    cookies
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.value.clone())
}

/// 挑选一个可用的远程调试端口（9222 起向上探测）。
fn pick_free_port() -> Result<u16> {
    for port in 9222..9300u16 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(anyhow!("no free port in range 9222-9299"))
}

/// 脱敏显示（保留前后 8 字符）。
fn mask(s: &str) -> String {
    if s.len() <= 16 {
        "****".to_string()
    } else {
        format!("{}...{}", &s[..8], &s[s.len() - 8..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_json_null_when_not_logged_in() {
        // 未登录时 localStorage 存的是字符串 "null"
        assert_eq!(parse_token_json("null").unwrap(), "");
        assert_eq!(parse_token_json("").unwrap(), "");
        // 或 DeepSeek 实际存的 {"value":null,"__version":"0"}
        assert_eq!(
            parse_token_json(r#"{"value":null,"__version":"0"}"#).unwrap(),
            ""
        );
    }

    #[test]
    fn parse_token_json_extracts_value() {
        let json = r#"{"value":"tok123","expireAt":1752000000}"#;
        assert_eq!(parse_token_json(json).unwrap(), "tok123");
    }

    #[test]
    fn parse_token_json_plain_string() {
        // 兼容某些场景下 localStorage 直接存字符串
        let json = r#""raw-token""#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        if let Some(s) = v.as_str() {
            assert_eq!(s, "raw-token");
        }
    }

    #[test]
    fn mask_shortens_long_tokens() {
        let m = mask("abcdefghijklmnopqrstuvwxyz");
        assert!(m.contains("..."));
        assert!(m.len() < 26);
    }

    #[test]
    fn normalize_lang_variants() {
        assert_eq!(normalize_lang("zh-CN"), "zh-CN");
        assert_eq!(normalize_lang("zh_CN"), "zh-CN");
        assert_eq!(normalize_lang("zh-cn"), "zh-CN");
        assert_eq!(normalize_lang("en"), "en");
        assert_eq!(normalize_lang("ja-JP"), "ja-JP");
    }

    #[test]
    fn derive_lang_from_locale_variants() {
        assert_eq!(derive_lang_from_locale("zh_CN.UTF-8").unwrap(), "zh-CN");
        assert_eq!(derive_lang_from_locale("en_US").unwrap(), "en-US");
        assert_eq!(derive_lang_from_locale("ja_JP.UTF-8").unwrap(), "ja-JP");
        assert_eq!(derive_lang_from_locale("C"), None);
        assert_eq!(derive_lang_from_locale("POSIX"), None);
        assert_eq!(derive_lang_from_locale(""), None);
    }

    #[test]
    fn accept_lang_builds_priority_list() {
        assert_eq!(accept_lang("zh-CN"), "zh-CN,zh;q=0.9,en;q=0.8");
        assert_eq!(accept_lang("en"), "en-US,en;q=0.9");
        assert_eq!(accept_lang("ja-JP"), "ja-JP,ja;q=0.9,en;q=0.8");
    }

    fn health(muted: bool, account_id: &str, label: &str) -> crate::account_health::AccountHealth {
        crate::account_health::AccountHealth {
            muted,
            mute_until: if muted { Some(1794738505) } else { None },
            account_id: account_id.to_string(),
            label: label.to_string(),
        }
    }

    /// 静音分支在交互模式下 SHALL NOT 是终态——浏览器保持打开、继续等换号。
    #[test]
    fn muted_in_interactive_mode_is_not_terminal() {
        let h = health(true, "acc-1", "a@b.c");
        let d = decide_next_step(Some(&h), LoginMode::Interactive, false);
        assert_eq!(d, LoginDecision::Muted { first_notice: true });
        assert_ne!(d, LoginDecision::MutedFatal);
        assert_ne!(d, LoginDecision::Healthy);
    }

    /// AutoRefresh 模式遇静音 SHALL 快速失败（无人值守，不等换号）。
    #[test]
    fn muted_in_auto_refresh_mode_fails_fast() {
        let h = health(true, "acc-1", "a@b.c");
        assert_eq!(
            decide_next_step(Some(&h), LoginMode::AutoRefresh, false),
            LoginDecision::MutedFatal
        );
    }

    /// 健康账号 SHALL 返回凭据（终态）。
    #[test]
    fn healthy_account_returns_credentials() {
        let h = health(false, "acc-1", "a@b.c");
        assert_eq!(
            decide_next_step(Some(&h), LoginMode::Interactive, false),
            LoginDecision::Healthy
        );
        assert_eq!(
            decide_next_step(Some(&h), LoginMode::AutoRefresh, false),
            LoginDecision::Healthy
        );
    }

    /// 探测失败 SHALL 降级为「未知健康」，SHALL NOT 中断等待。
    #[test]
    fn probe_failure_degrades_to_unknown() {
        assert_eq!(
            decide_next_step(None, LoginMode::Interactive, false),
            LoginDecision::Unknown
        );
        assert_eq!(
            decide_next_step(None, LoginMode::AutoRefresh, false),
            LoginDecision::Unknown
        );
    }

    /// 两段式换号判定：同一 account_id 刷新 token 不重复提示；换新账号允许再提示。
    #[test]
    fn same_account_token_refresh_does_not_repeat_notice() {
        let mut hinted: HashSet<String> = HashSet::new();

        let h1 = health(true, "acc-1", "a@b.c");
        let first = decide_next_step(
            Some(&h1),
            LoginMode::Interactive,
            hinted.contains(&h1.account_id),
        );
        assert_eq!(first, LoginDecision::Muted { first_notice: true });
        assert!(mark_muted_notice(&mut hinted, &h1.account_id));

        // 同账号换了 token（探测仍返回同一 account_id）→ 不再提示
        let h2 = health(true, "acc-1", "a@b.c");
        let second = decide_next_step(
            Some(&h2),
            LoginMode::Interactive,
            hinted.contains(&h2.account_id),
        );
        assert_eq!(
            second,
            LoginDecision::Muted {
                first_notice: false
            }
        );
        assert!(!mark_muted_notice(&mut hinted, &h2.account_id));

        // 换新账号 → 允许再次提示
        let h3 = health(true, "acc-2", "x@y.z");
        let third = decide_next_step(
            Some(&h3),
            LoginMode::Interactive,
            hinted.contains(&h3.account_id),
        );
        assert_eq!(third, LoginDecision::Muted { first_notice: true });
        assert!(mark_muted_notice(&mut hinted, &h3.account_id));
    }

    /// 超时错误 SHALL 携带静音截止时间。
    #[test]
    fn timeout_error_mentions_mute_deadline() {
        let e = login_timeout_error(Duration::from_secs(600), Some(1794738505));
        let text = e.to_string();
        assert!(text.contains("600 seconds"));
        assert!(text.contains("2026-11-15 10:28 UTC"));

        let e2 = login_timeout_error(Duration::from_secs(90), None);
        assert!(e2.to_string().contains("90 seconds"));
        assert!(!e2.to_string().contains("muted account"));
    }

    /// 两个通道的换号指引文案 SHALL 一致（CLI stderr 与 MCP 返回文本共用同一份）。
    #[test]
    fn guidance_text_is_shared_by_both_channels() {
        let text = muted_guidance("a@b.c", "acc-1", Some(1794738505));
        assert!(text.contains("a@b.c"));
        assert!(text.contains("acc-1"));
        assert!(text.contains("2026-11-15 10:28 UTC"));
        assert!(text.contains("MUTED"));

        let banner = muted_banner_html("a@b.c", "acc-1", Some(1794738505));
        assert!(banner.contains("a@b.c"));
        assert!(banner.contains("2026-11-15 10:28 UTC"));
        // 横幅 SHALL 明确引导「换号登录」、分行且强调动作
        assert!(banner.contains("换号登录"));
        assert!(banner.contains("请在本窗口先登出"));
        assert!(banner.contains("<br>"));
        assert!(banner.contains("<strong>"));
        // 未知截止时间时也要给出可读兜底
        let unknown = muted_banner_html("a@b.c", "acc-1", None);
        assert!(unknown.contains("未知截止时间"));
    }

    /// 横幅 SHALL NOT 拦截页面交互：容器 `pointer-events: none`，
    /// 只有「×」按钮 `pointer-events: auto`。
    ///
    /// 这条是硬需求——横幅覆盖在页面顶部，若拦截点击，用户就点不动
    /// 登出/换号等关键按钮。
    #[test]
    fn banner_never_blocks_page_interaction() {
        let js = muted_banner_script("hello");
        // 容器不拦截
        assert!(js.contains("pointer-events:none"), "{js}");
        // 仅关闭按钮可点
        assert!(js.contains("pointer-events:auto"), "{js}");
        // 不再是顶部通栏（会盖住页面顶部按钮）
        assert!(!js.contains("top:0;left:0;right:0"), "{js}");
        // 提供关闭按钮 + 关闭后本会话不再自动弹回
        assert!(js.contains("sessionStorage"), "{js}");
        assert!(js.contains("关闭静音提示"), "{js}");
        // 正文与按钮分离，刷新正文不会抹掉关闭按钮
        assert!(js.contains(BANNER_BODY_ID), "{js}");
    }

    /// 模式区分 SHALL 由显式枚举承担，SHALL NOT 依赖超时数值。
    #[test]
    fn modes_are_distinguished_by_enum_not_timeout() {
        assert_ne!(LoginMode::Interactive, LoginMode::AutoRefresh);
        let h = health(true, "acc-1", "a@b.c");
        assert_eq!(
            decide_next_step(Some(&h), LoginMode::Interactive, false),
            LoginDecision::Muted { first_notice: true }
        );
        assert_eq!(
            decide_next_step(Some(&h), LoginMode::AutoRefresh, false),
            LoginDecision::MutedFatal
        );
    }
}

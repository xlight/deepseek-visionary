//! Vision 主流水线（对照 Python 版 `server.py::_run_vision_pipeline`）。
//!
//! 流程：上传(含 PoW + 轮询) → 创建/复用会话 → HIF 签名 →
//! completion(含 PoW + SSE 流式)，返回 (回答文本, session_id, parent_message_id)。
//!
//! 上传管道由 `model_type` 决定：`None`/`"vision"` 携带 `x-model-type: vision`
//! （上传即产出视觉可用文件，不再调用 `fork_file_task`，见 upload.rs）；`"ocr"`
//! 不携带（走服务端 OCR 文本提取管道，文字不足时返回 CONTENT_EMPTY 提示）。

use crate::client::ApiClient;
use crate::completion;
use crate::config::{self, Config};
use crate::hif::HifAuth;
use crate::session::{self, SessionStore};
use crate::upload;
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use std::io::Read as _;

/// 单次 vision 分析的输出。
pub struct PipelineOutput {
    pub text: String,
    pub session_id: String,
    /// 供会话续聊的新父消息 id；已由 `SessionStore` 持久化消费，
    /// 保留字段供未来向客户端暴露。
    #[allow(dead_code)]
    pub parent_message_id: Option<String>,
}

/// vision 管道默认提示词（与 CLI `vision` 子命令 / MCP 工具默认一致）。
pub const VISION_DEFAULT_PROMPT: &str = "请详细描述这张图片中的内容";
/// ocr 管道默认提示词：要求原样输出图片文字（可被用户 prompt 覆盖）。
pub const OCR_DEFAULT_PROMPT: &str = "请原样输出图片中的文字内容";

/// CONTENT_EMPTY 短路的纯决策：OCR 管道下是"未提取到文字"业务提示（短路）；
/// vision 管道保持放行兜底（让 completion 自然报错传播）。
fn content_empty_action(is_ocr: bool) -> Result<()> {
    if is_ocr {
        Err(anyhow!("图片中未提取到文字（服务端 OCR 提取为空）"))
    } else {
        Ok(())
    }
}

/// vision 流水线输入参数。
pub struct VisionRequest {
    /// 多张图片字节（一次上传、联合分析，与网页端多图行为一致）。
    pub images_data: Vec<Vec<u8>>,
    pub prompt: String,
    pub thinking: bool,
    /// 显式复用的 session_id（对应工具参数 `session_id`）。
    pub session_id: Option<String>,
    /// 续聊的父消息 id（对应 `continue_conversation` 场景）。
    ///
    /// 调用方 SHALL NOT 预填：会话解析在流水线内部、**选号之后**完成
    /// （换号后父消息 id 不得跨账号沿用）。该字段保留仅为兼容既有结构。
    #[allow(dead_code)]
    pub parent_message_id: Option<String>,
    /// 上传管道：`None`/`"vision"`（默认 vision 管道）| `Some("ocr")`（OCR 管道）。
    pub model_type: Option<String>,
}

/// 读取图片字节（MCP `deepseek_vision` handler 与 CLI `vision` 子命令共用）。
///
/// 支持四种形态（对齐 Python 版读取逻辑）：
/// - 本地路径（相对路径按进程 cwd 解析）
/// - `-`：从 stdin 读取全部字节（CLI 专用，管道输入）
/// - base64 编码字符串（长度 >100 才探测，避免把短路径误判）
/// - data URI（`data:...;base64,...`）
pub fn read_image(image: &str) -> Result<Vec<u8>, String> {
    if image == "-" {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read image from stdin: {e}"))?;
        return Ok(buf);
    }
    if image.starts_with("data:") || is_base64(image) {
        let encoded = image.split_once(',').map(|(_, e)| e).unwrap_or(image);
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("Failed to decode base64 image: {e}"))
    } else {
        std::fs::read(image).map_err(|e| format!("Failed to read image `{image}`: {e}"))
    }
}

/// base64 探测（对应 Python `_is_base64`：长度 >100 且可解码）。
pub fn is_base64(s: &str) -> bool {
    if s.len() <= 100 {
        return false;
    }
    base64::engine::general_purpose::STANDARD.decode(s).is_ok()
}

/// 会话续聊解析（由流水线内部统一调用，调用方不再预解析）。
///
/// 规则与 Python 版对齐：显式 `session_id` 优先于 `continue_conversation`；
/// 显式 session_id 无本地持久化记录时仅复用该 id、不携带 parent_message_id。
/// 返回 `(session_id, parent_message_id)`，两者皆 `None` 表示新建会话。
///
/// `account_id` 决定读哪个账号的会话桶：会话绑定账号（换号即断链），
/// 轮询到别的账号时 SHALL NOT 命中前一账号的会话记录。
pub fn resolve_session_reuse(
    session_store: &SessionStore,
    account_id: &str,
    session_id: Option<&str>,
    continue_conversation: bool,
) -> (Option<String>, Option<String>) {
    if let Some(sid) = session_id {
        let saved = session_store.load(account_id);
        let parent = saved
            .filter(|s| s.session_id.as_deref() == Some(sid))
            .and_then(|s| s.parent_message_id);
        (Some(sid.to_string()), parent)
    } else if continue_conversation {
        let saved = session_store.load(account_id);
        match saved {
            Some(s) => (s.session_id, s.parent_message_id),
            None => (None, None),
        }
    } else {
        (None, None)
    }
}

/// 旧格式（平铺）会话的归属账号：启用账号池时归入「启用池时当前使用的账号」的桶。
///
/// 「当前使用的账号」= 兼容配置 `config.json` 里那组凭据对应的池账号——
/// 登记写回会把最后登记的账号写进 config.json，因此它就是当时的当前账号。
/// 找不到对应账号时返回 None（归入默认桶），SHALL NOT 造占位键。
pub fn legacy_session_owner(config: &Config) -> Option<String> {
    if !config.pool_mode() {
        return None;
    }
    let creds = config.credentials();
    if creds.user_token.is_empty() {
        return None;
    }
    config
        .pool()
        .accounts()
        .iter()
        .find(|a| a.user_token == creds.user_token)
        .map(|a| a.account_id.clone())
}

/// ocr 管道默认提示词：用户未显式覆盖（prompt 恰为 vision 默认）时改用文字提取语义。
pub fn resolve_prompt(prompt: String, is_ocr: bool) -> String {
    if is_ocr && prompt == VISION_DEFAULT_PROMPT {
        OCR_DEFAULT_PROMPT.to_string()
    } else {
        prompt
    }
}

/// 执行完整 vision 流水线。
///
/// `on_token` 为可选流式回调（透传给 completion）：CLI 流式打印用，`None` 时仅收集。
///
/// **单账号模式**（无账号池）：行为与 v0.7.8 一致——token 失效（DeepSeek 会轮换
/// `userToken`）时自动续期并重跑一次（续期会短暂拉起一次浏览器）。
///
/// **账号池模式**：每次分析请求前按轮询策略选一个账号、仅在内存里热替换凭据；
/// 当前账号被判定静音或鉴权失败时标记其不可用、自动换下一个账号重试
///（池模式下**不**拉起浏览器——换号就是换凭据）。
///
/// 会话解析在**选号之后**统一完成：会话绑定账号，换号后 SHALL NOT 沿用前一账号的
/// 父消息 id（显式传入的 session_id 原样复用）。
pub async fn run_vision_pipeline<F>(
    config: &Config,
    hif: &HifAuth,
    session_store: &SessionStore,
    request: VisionRequest,
    continue_conversation: bool,
    mut on_token: Option<F>,
) -> Result<PipelineOutput>
where
    F: FnMut(&str) + Send,
{
    if config.pool_mode() {
        return run_pool_pipeline(
            config,
            hif,
            session_store,
            request,
            continue_conversation,
            on_token,
        )
        .await;
    }

    // 单账号模式：会话归入默认桶（与既有行为一致）。
    let bucket = crate::session::DEFAULT_BUCKET.to_string();
    let (session_id, parent_message_id) = resolve_session_reuse(
        session_store,
        &bucket,
        request.session_id.as_deref(),
        continue_conversation,
    );
    let mut refreshed = false;
    loop {
        match run_pipeline_once(
            config,
            hif,
            session_store,
            &request,
            &bucket,
            session_id.clone(),
            parent_message_id.clone(),
            on_token.as_mut(),
        )
        .await
        {
            Ok(output) => return Ok(output),
            Err(e) => {
                // 静音（`MutedFailure`）SHALL NOT 触发自动续期：对静音账号刷新 token
                // 完全无效（token 有效，只是账号被限），拉起浏览器只会再撞一次静音。
                if e.downcast_ref::<crate::client::MutedFailure>().is_some() {
                    tracing::warn!("account is muted; skipping token auto-refresh");
                    return Err(e);
                }
                let auth_failed = e.downcast_ref::<crate::client::AuthFailure>().is_some();
                if !auth_failed || refreshed {
                    return Err(e);
                }
                refreshed = true;
                tracing::warn!(
                    "token expired during the pipeline; refreshing credentials and retrying once"
                );
                crate::auth::refresh_token(config)
                    .await
                    .context("token auto-refresh failed")?;
                // HIF 签名是用旧 token 换来的，续期后必须作废缓存。
                hif.invalidate_all();
            }
        }
    }
}

/// 账号池模式下的流水线：选号 → 热替换凭据 → 解析会话 → 执行；
/// 账号级失败（静音 / 凭据失效）时标记该账号不可用并换下一个账号重试。
///
/// 瞬时错误（网络抖动 / 超时）SHALL NOT 标记账号不可用——它反映链路而非账号状态，
/// 计入账号级失败会迅速耗尽整个池。
async fn run_pool_pipeline<F>(
    config: &Config,
    hif: &HifAuth,
    session_store: &SessionStore,
    request: VisionRequest,
    continue_conversation: bool,
    mut on_token: Option<F>,
) -> Result<PipelineOutput>
where
    F: FnMut(&str) + Send,
{
    let pool_path = crate::accounts::pool_file()?;
    let max_attempts = {
        let pool = config.pool();
        pool.accounts().len().max(1)
    };
    let mut last_err: Option<anyhow::Error> = None;

    for _ in 0..max_attempts {
        // 选号（游标在池内推进；固定账号直接命中且不推进游标）。
        let selection = {
            let mut pool = config
                .pool
                .write()
                .map_err(|e| anyhow!("account pool lock poisoned: {e}"))?;
            pool.next_usable()
        };
        let account = match selection {
            crate::accounts::AccountSelection::Selected(a) => *a,
            crate::accounts::AccountSelection::PinnedUnavailable { label, reason } => {
                // 固定账号不可用时 SHALL 仍只用它（固定是用户的排障手段），
                // 静默回退轮询会让排障结论失效。
                return Err(anyhow!(
                    "固定账号 {label} 当前不可用：{}。固定账号不会自动换号——请修复该账号，或用 `accounts unpin` 取消固定。",
                    reason.message()
                ));
            }
            crate::accounts::AccountSelection::None => {
                return Err(no_usable_account_error());
            }
        };

        // 热替换凭据：**仅内存**，SHALL NOT 写 config.json（轮询期切换不是"登记"）。
        config.reload_credentials(account.credentials());

        // 选号写回：持久化游标（跨进程轮换）+ 节流更新 last_used。
        {
            let mut pool = config
                .pool
                .write()
                .map_err(|e| anyhow!("account pool lock poisoned: {e}"))?;
            if let Err(e) =
                crate::accounts::persist_selection(&pool_path, &mut pool, &account.account_id)
            {
                tracing::warn!("failed to persist account selection: {e:#}");
            }
        }

        // 会话解析在选号之后：换号后不沿用前一账号的父消息 id。
        let (session_id, parent_message_id) = resolve_session_reuse(
            session_store,
            &account.account_id,
            request.session_id.as_deref(),
            continue_conversation,
        );

        match run_pipeline_once(
            config,
            hif,
            session_store,
            &request,
            &account.account_id,
            session_id,
            parent_message_id,
            on_token.as_mut(),
        )
        .await
        {
            Ok(output) => return Ok(output),
            Err(e) => {
                let Some(reason) = account_failure_reason(&e) else {
                    // 瞬时错误：不换号、不标记账号不可用。
                    return Err(e);
                };
                tracing::warn!(
                    "account {} unavailable ({}); switching to the next account",
                    account.label,
                    reason.message()
                );
                // 标记不可用并落盘（重启后不再重复撞同一个坏账号）。
                {
                    let mut pool = config
                        .pool
                        .write()
                        .map_err(|e| anyhow!("account pool lock poisoned: {e}"))?;
                    pool.mark_unusable(&account.account_id, reason.clone());
                }
                if let Err(write_err) = crate::accounts::merge_and_save(&pool_path, |pool| {
                    pool.mark_unusable(&account.account_id, reason.clone());
                    Ok(())
                }) {
                    tracing::warn!("failed to persist account status: {write_err:#}");
                }
                // HIF 签名与账号绑定：只作废该账号的缓存。
                hif.invalidate(&account.account_id);
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(no_usable_account_error))
}

/// 账号级失败分类：把流水线错误映射成「账号不可用原因」。
///
/// - 静音（`MutedFailure`）→ 静音原因（带截止时间）
/// - 鉴权失败（`AuthFailure`）→ 凭据失效
/// - 其它（网络抖动 / 超时 / 业务错误）→ `None`：**瞬时错误不消耗账号**，
///   它反映链路而非账号状态，计入账号级失败会迅速耗尽整个池。
fn account_failure_reason(err: &anyhow::Error) -> Option<crate::accounts::UnusableReason> {
    if let Some(m) = err.downcast_ref::<crate::client::MutedFailure>() {
        return Some(crate::accounts::UnusableReason::Muted {
            until: m.mute_until,
        });
    }
    if err.downcast_ref::<crate::client::AuthFailure>().is_some() {
        return Some(crate::accounts::UnusableReason::Expired);
    }
    None
}

/// 「账号池中没有可用账号」的标准错误。
fn no_usable_account_error() -> anyhow::Error {
    anyhow!(
        "账号池中没有可用账号：请运行 `visionary-server accounts list` 查看状态，或 `accounts add` 添加账号。"
    )
}

/// 单次流水线执行（不含 token 续期重试、不含换号重试）。
///
/// `account_id` 决定会话桶与凭据来源；`session_id` / `parent_message_id` 是
/// **选号之后**解析出来的会话上下文（调用方不得预填）。
#[allow(clippy::too_many_arguments)]
async fn run_pipeline_once<F>(
    config: &Config,
    hif: &HifAuth,
    session_store: &SessionStore,
    request: &VisionRequest,
    account_id: &str,
    session_id: Option<String>,
    parent_message_id: Option<String>,
    on_token: Option<&mut F>,
) -> Result<PipelineOutput>
where
    F: FnMut(&str) + Send,
{
    let client = ApiClient::new(config.clone())?;

    if request.images_data.is_empty() {
        return Err(anyhow!("vision request requires at least one image"));
    }

    let model_type = request.model_type.as_deref();
    let is_ocr = model_type == Some(config::MODEL_TYPE_OCR);

    // ocr 管道默认提示词：用户未显式覆盖（prompt 恰为 vision 默认）时改用文字提取语义。
    let prompt = resolve_prompt(request.prompt.clone(), is_ocr);

    // Step 1: 上传所有图片（vision 管道携带 x-model-type: vision，上传即视觉可用；
    // ocr 管道不携带，走服务端 OCR 文本提取）+ 轮询 SUCCESS
    tracing::info!(
        "Step 1: uploading {} image(s)... (model_type={})",
        request.images_data.len(),
        model_type.unwrap_or(config::MODEL_TYPE_VISION)
    );
    let mut file_infos = Vec::with_capacity(request.images_data.len());
    for image_data in &request.images_data {
        let file_info = upload::upload_and_wait(&client, image_data.clone(), model_type).await?;
        tracing::info!("  uploaded: {} (status={})", file_info.id, file_info.status);
        // CONTENT_EMPTY：OCR 管道下是"图片中未提取到文字"的业务提示（无文字/文字太少），
        // 直接短路返回；vision 管道保持现状兜底（放行，让 completion 自然报错传播）。
        if file_info.status == "CONTENT_EMPTY" {
            // content_empty_action 返回 Err(业务提示) 让 ocr 短路，返回 Ok(()) 让 vision 放行
            content_empty_action(is_ocr)?;
            tracing::warn!(
                "File {} OCR extraction empty (CONTENT_EMPTY) under vision pipeline; proceeding",
                file_info.id
            );
        }
        file_infos.push(file_info);
    }

    // Step 2: 上传文件即视觉可用（x-model-type: vision），直接收集 file id，
    // 不再 fork（网页端已移除 fork_file_task 流程）
    let vision_file_ids: Vec<String> = file_infos.iter().map(|f| f.id.clone()).collect();

    // Step 3: 创建或复用会话（会话 id 已由选号后的解析给出）
    let session_id = if let Some(sid) = session_id {
        tracing::info!("Step 3: reusing session: {sid}");
        sid
    } else {
        tracing::info!("Step 3: creating session...");
        let sid = session::create_session(&client).await?;
        tracing::info!("  session: {sid}");
        sid
    };

    // Step 4+5: HIF 签名 + vision completion（多图一次送入 ref_file_ids）
    tracing::info!(
        "Step 4/5: vision completion ({} files)...",
        vision_file_ids.len()
    );
    let (text, new_parent_message_id) = completion::vision_completion(
        &client,
        hif,
        account_id,
        &session_id,
        &vision_file_ids,
        &prompt,
        request.thinking,
        parent_message_id.as_deref(),
        on_token,
    )
    .await
    .context("vision completion")?;

    // 持久化会话状态（按账号分桶，供 continue_conversation / session_id 复用）
    session_store.save(
        account_id,
        &session::SessionState {
            session_id: Some(session_id.clone()),
            parent_message_id: new_parent_message_id.clone(),
        },
    );

    Ok(PipelineOutput {
        text,
        session_id,
        parent_message_id: new_parent_message_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 静音错误 → 账号级失败（带静音截止时间）。
    #[test]
    fn muted_error_is_account_failure() {
        let err: anyhow::Error = crate::client::MutedFailure::new("muted", Some(1794738505)).into();
        let reason = account_failure_reason(&err).expect("muted must be an account failure");
        assert_eq!(
            reason,
            crate::accounts::UnusableReason::Muted {
                until: Some(1794738505)
            }
        );
    }

    /// 鉴权失败 → 账号级失败（凭据失效）。
    #[test]
    fn auth_failure_is_account_failure() {
        let err: anyhow::Error = crate::client::AuthFailure("invalid token".into()).into();
        let reason = account_failure_reason(&err).expect("auth failure must be an account failure");
        assert_eq!(reason, crate::accounts::UnusableReason::Expired);
    }

    /// 瞬时错误（网络 / 超时）**不是**账号级失败：不换号、不标记账号不可用。
    #[test]
    fn transient_error_is_not_account_failure() {
        let err: anyhow::Error = anyhow!("connection reset by peer");
        assert!(account_failure_reason(&err).is_none());
    }

    /// 「没有可用账号」的错误文案引导用户去看账号池。
    #[test]
    fn no_usable_account_error_points_to_pool() {
        let msg = no_usable_account_error().to_string();
        assert!(msg.contains("accounts list"), "unexpected: {msg}");
    }

    #[test]
    fn ocr_content_empty_returns_hint() {
        // OCR 管道 CONTENT_EMPTY → 业务提示错误（CLI 经 fail() 退出非零，MCP 经 error 呈现）。
        let err = content_empty_action(true).unwrap_err();
        assert!(
            err.to_string().contains("未提取到文字"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn vision_content_empty_still_proceeds() {
        // vision 管道 CONTENT_EMPTY → 放行（兜底），不报错。
        assert!(content_empty_action(false).is_ok());
    }

    #[test]
    fn ocr_default_prompt_swaps_vision_default() {
        // ocr 管道：prompt 恰为 vision 默认 → 替换为文字提取语义。
        assert_eq!(
            resolve_prompt(VISION_DEFAULT_PROMPT.to_string(), true),
            OCR_DEFAULT_PROMPT
        );
        // 用户显式给出自定义 prompt → 保留（可覆盖）。
        assert_eq!(resolve_prompt("只提取表格".to_string(), true), "只提取表格");
        // vision 管道：默认与自定义均不变。
        assert_eq!(
            resolve_prompt(VISION_DEFAULT_PROMPT.to_string(), false),
            VISION_DEFAULT_PROMPT
        );
        assert_eq!(
            resolve_prompt("自定义问题".to_string(), false),
            "自定义问题"
        );
    }
}

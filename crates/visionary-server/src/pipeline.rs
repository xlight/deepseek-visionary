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

/// 会话续聊解析（MCP `deepseek_vision` handler 与 CLI `vision` 子命令共用）。
///
/// 规则与 Python 版对齐：显式 `session_id` 优先于 `continue_conversation`；
/// 显式 session_id 无本地持久化记录时仅复用该 id、不携带 parent_message_id。
/// 返回 `(session_id, parent_message_id)`，两者皆 `None` 表示新建会话。
pub fn resolve_session_reuse(
    session_store: &SessionStore,
    session_id: Option<&str>,
    continue_conversation: bool,
) -> (Option<String>, Option<String>) {
    if let Some(sid) = session_id {
        let saved = session_store.load();
        let parent = saved
            .filter(|s| s.session_id.as_deref() == Some(sid))
            .and_then(|s| s.parent_message_id);
        (Some(sid.to_string()), parent)
    } else if continue_conversation {
        let saved = session_store.load();
        match saved {
            Some(s) => (s.session_id, s.parent_message_id),
            None => (None, None),
        }
    } else {
        (None, None)
    }
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
pub async fn run_vision_pipeline<F>(
    config: &Config,
    hif: &HifAuth,
    session_store: &SessionStore,
    request: VisionRequest,
    on_token: Option<F>,
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
    let prompt = resolve_prompt(request.prompt, is_ocr);

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

    // Step 3: 创建或复用会话
    let session_id = if let Some(sid) = &request.session_id {
        tracing::info!("Step 3: reusing session: {sid}");
        sid.clone()
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
        &session_id,
        &vision_file_ids,
        &prompt,
        request.thinking,
        request.parent_message_id.as_deref(),
        on_token,
    )
    .await
    .context("vision completion")?;

    // 持久化会话状态（供 continue_conversation / session_id 复用）
    session_store.save(&session::SessionState {
        session_id: Some(session_id.clone()),
        parent_message_id: new_parent_message_id.clone(),
    });

    Ok(PipelineOutput {
        text,
        session_id,
        parent_message_id: new_parent_message_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

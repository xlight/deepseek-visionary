//! Vision 主流水线（对照 Python 版 `server.py::_run_vision_pipeline`）。
//!
//! 流程：上传(含 PoW + 轮询) → fork 到 vision → 创建/复用会话 → HIF 签名 →
//! completion(含 PoW + SSE 流式)，返回 (回答文本, session_id, parent_message_id)。

use crate::client::ApiClient;
use crate::completion;
use crate::config::Config;
use crate::fork;
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

/// vision 流水线输入参数。
pub struct VisionRequest {
    /// 多张图片字节（一次上传、fork、联合分析，与网页端多图行为一致）。
    pub images_data: Vec<Vec<u8>>,
    pub prompt: String,
    pub thinking: bool,
    /// 显式复用的 session_id（对应工具参数 `session_id`）。
    pub session_id: Option<String>,
    /// 续聊的父消息 id（对应 `continue_conversation` 场景）。
    pub parent_message_id: Option<String>,
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

    // Step 1: 上传所有图片（OCR）+ 轮询 SUCCESS
    tracing::info!(
        "Step 1: uploading {} image(s)...",
        request.images_data.len()
    );
    let mut file_infos = Vec::with_capacity(request.images_data.len());
    for image_data in &request.images_data {
        let file_info = upload::upload_and_wait(&client, image_data.clone()).await?;
        tracing::info!("  uploaded: {} (OCR)", file_info.id);
        file_infos.push(file_info);
    }

    // Step 2: 每张图 fork 到 vision 模型，收集 vision file id
    tracing::info!("Step 2: forking to vision model...");
    let mut vision_file_ids = Vec::with_capacity(file_infos.len());
    for file_info in &file_infos {
        let vision_file_id = fork::fork_to_vision(&client, &file_info.id).await?;
        tracing::info!("  vision file: {vision_file_id}");
        vision_file_ids.push(vision_file_id);
    }

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
        &request.prompt,
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

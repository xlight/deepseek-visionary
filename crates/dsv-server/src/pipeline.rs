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
use anyhow::{Context, Result};

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
    pub image_data: Vec<u8>,
    pub prompt: String,
    pub thinking: bool,
    /// 显式复用的 session_id（对应工具参数 `session_id`）。
    pub session_id: Option<String>,
    /// 续聊的父消息 id（对应 `continue_conversation` 场景）。
    pub parent_message_id: Option<String>,
}

/// 执行完整 vision 流水线。
pub async fn run_vision_pipeline(
    config: &Config,
    hif: &HifAuth,
    session_store: &SessionStore,
    request: VisionRequest,
) -> Result<PipelineOutput> {
    let client = ApiClient::new(config.clone())?;

    // Step 1: 上传（OCR）+ 轮询 SUCCESS
    tracing::info!("Step 1: uploading image...");
    let file_info = upload::upload_and_wait(&client, request.image_data).await?;
    tracing::info!("  uploaded: {} (OCR)", file_info.id);

    // Step 2: fork 到 vision 模型
    tracing::info!("Step 2: forking to vision model...");
    let vision_file_id = fork::fork_to_vision(&client, &file_info.id).await?;
    tracing::info!("  vision file: {vision_file_id}");

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

    // Step 4+5: HIF 签名 + vision completion
    tracing::info!("Step 4/5: vision completion...");
    let (text, new_parent_message_id) = completion::vision_completion(
        &client,
        hif,
        &session_id,
        &vision_file_id,
        &request.prompt,
        request.thinking,
        request.parent_message_id.as_deref(),
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

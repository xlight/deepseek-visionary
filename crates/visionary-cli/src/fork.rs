//! 将上传文件 fork 到 vision 模型（对照 Python 版 `server.py::_fork_to_vision`）。
//!
//! 上传后的文件只做了 OCR 文本提取；fork 会创建一份具备完整图像理解能力的
//! 新文件（vision model），随后轮询其状态直到 SUCCESS。

use crate::client::ApiClient;
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// Fork 到 vision 模型并等待处理完成，返回 vision file_id。
pub async fn fork_to_vision(client: &ApiClient, file_id: &str) -> Result<String> {
    let envelope = client
        .post_json(
            "/api/v0/file/fork_file_task",
            &serde_json::json!({
                "file_id": file_id,
                "to_model_type": "vision",
            }),
            Duration::from_secs(30),
        )
        .await?;
    let biz = envelope.into_biz_data("fork")?;
    let vision_file_id = biz
        .get("id")
        .and_then(|v| v.as_str())
        .context("fork response missing id")?
        .to_string();

    // 等待 vision 处理完成（上限 30s，对应 Python 的 `for i in range(30)`）
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let envelope = client
            .get_json(
                "/api/v0/file/fetch_files",
                &[("file_ids", vision_file_id.clone())],
            )
            .await?;
        let biz = envelope.into_biz_data("fetch_files")?;
        let files = biz
            .get("files")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(first) = files.first() {
            let status = first
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            if status == "SUCCESS" {
                return Ok(vision_file_id);
            }
            if status == "FAILED" || status == "ERROR" {
                return Err(anyhow!("Vision file processing failed: {status}"));
            }
        }
    }

    Err(anyhow!("Vision file did not become ready within 30s"))
}

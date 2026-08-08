//! 图片上传与状态轮询（对照 Python 版 `upload.py`）。
//!
//! 流水线：图片压缩 → 获取并求解上传 PoW → multipart 上传 → 轮询状态至 SUCCESS。

use crate::client::ApiClient;
use crate::pow::{self, Challenge};
use anyhow::{anyhow, Context, Result};
use std::io::Cursor;
use std::time::{Duration, Instant};

/// 上传成功后返回的文件信息（对应 Python `FileInfo`）。
#[derive(Debug, Clone)]
/// 上传成功后返回的文件信息（对应 Python `FileInfo`）。
/// 当前流水线消费 `id`；其余字段与 Python 对齐保留，供未来 status/审计使用。
#[allow(dead_code)]
pub struct FileInfo {
    pub id: String,
    pub file_name: String,
    pub file_size: i64,
    pub status: String,
    pub is_image: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub signed_path: Option<String>,
    pub audit_result: Option<String>,
}

/// 上传文件并等待处理完成。
pub async fn upload_and_wait(client: &ApiClient, image_data: Vec<u8>) -> Result<FileInfo> {
    let image_data = maybe_compress(&image_data)?;

    // 上传前 PoW（对应 Python `_get_pow_challenge` + `solve_challenge`）
    let challenge = create_pow_challenge(client, "/api/v0/file/upload_file").await?;
    let pow_header = pow::PoWSolver::solve_challenge(&challenge)?;

    let headers = vec![
        ("x-ds-pow-response".into(), pow_header),
        ("Accept".into(), "application/json".into()),
    ];

    let envelope = client
        .post_multipart(
            "/api/v0/file/upload_file",
            "file",
            "image.png",
            "image/png",
            image_data,
            headers,
            client.config.upload_timeout,
        )
        .await?;

    let biz = envelope.into_biz_data("upload")?;
    let file_data = biz
        .as_object()
        .context("upload biz_data is not an object")?;
    let file_info = parse_file_info(file_data)?;

    // 轮询直到 SUCCESS
    wait_for_success(client, &file_info.id, None).await
}

/// 获取并返回 PoW challenge（对应 Python `_get_pow_challenge`）。
pub async fn create_pow_challenge(client: &ApiClient, target_path: &str) -> Result<Challenge> {
    let envelope = client
        .post_json(
            "/api/v0/chat/create_pow_challenge",
            &serde_json::json!({ "target_path": target_path }),
            Duration::from_secs(30),
        )
        .await?;
    let biz = envelope.into_biz_data("pow challenge")?;
    let challenge = biz
        .get("challenge")
        .cloned()
        .context("pow challenge response missing `challenge`")?;
    serde_json::from_value(challenge).context("parse pow challenge")
}

/// 轮询文件状态直到 SUCCESS / 失败终态 / 超时（对应 Python `wait_for_success`）。
pub async fn wait_for_success(
    client: &ApiClient,
    file_id: &str,
    timeout: Option<Duration>,
) -> Result<FileInfo> {
    let timeout = timeout.unwrap_or(client.config.poll_timeout);
    let interval = client.config.poll_interval;
    let deadline = Instant::now() + timeout;

    let terminal_failures = [
        "FAILED",
        "CONTENT_EMPTY",
        "CONTENT_FILTER",
        "CONTENT_TOO_LONG",
        "CANCELLED",
    ];

    let mut last_status = "PENDING".to_string();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!(
                "File {file_id} did not reach SUCCESS within {}s (last status: {last_status})",
                timeout.as_secs()
            ));
        }

        let info = fetch_file(client, file_id).await?;
        last_status = info.status.clone();

        if info.status == "SUCCESS" {
            return Ok(info);
        }
        if terminal_failures.contains(&info.status.as_str()) {
            return Err(anyhow!(
                "File {file_id} processing failed: status={}",
                info.status
            ));
        }
        tokio::time::sleep(interval.min(deadline - now)).await;
    }
}

/// 获取单个文件状态（对应 Python `_fetch_file`）。
pub async fn fetch_file(client: &ApiClient, file_id: &str) -> Result<FileInfo> {
    let envelope = client
        .get_json("/api/v0/file/fetch_files", &[("file_ids", file_id.into())])
        .await?;
    let biz = envelope.into_biz_data("fetch_files")?;
    let files = biz
        .get("files")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(first) = files.first() else {
        return Ok(FileInfo {
            id: file_id.into(),
            ..FileInfo::default()
        });
    };
    parse_file_info(first.as_object().context("file is not an object")?)
}

fn parse_file_info(obj: &serde_json::Map<String, serde_json::Value>) -> Result<FileInfo> {
    Ok(FileInfo {
        id: obj
            .get("id")
            .and_then(|v| v.as_str())
            .context("file missing id")?
            .to_string(),
        file_name: obj
            .get("file_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        file_size: obj.get("file_size").and_then(|v| v.as_i64()).unwrap_or(0),
        status: obj
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("PENDING")
            .to_uppercase(),
        is_image: obj
            .get("is_image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        width: obj.get("width").and_then(|v| v.as_i64()),
        height: obj.get("height").and_then(|v| v.as_i64()),
        signed_path: obj
            .get("signed_path")
            .and_then(|v| v.as_str())
            .map(String::from),
        audit_result: obj
            .get("audit_result")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

impl Default for FileInfo {
    fn default() -> Self {
        Self {
            id: String::new(),
            file_name: String::new(),
            file_size: 0,
            status: "PENDING".into(),
            is_image: false,
            width: None,
            height: None,
            signed_path: None,
            audit_result: None,
        }
    }
}

/// 图片压缩（对应 Python `_maybe_compress`）：
/// 宽高均 ≤ 2048 且大小 ≤ 20MB 时原样返回；否则等比缩放（LANCZOS）、
/// RGBA→RGB、PNG optimize 重新编码。
pub fn maybe_compress(image_data: &[u8]) -> Result<Vec<u8>> {
    const MAX_DIM: u32 = 2048;
    const MAX_BYTES: usize = 20 * 1024 * 1024;

    let img = match image::load_from_memory(image_data) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("image decode failed, using original: {e}");
            return Ok(image_data.to_vec());
        }
    };
    let (w, h) = (img.width(), img.height());
    if w <= MAX_DIM && h <= MAX_DIM && image_data.len() <= MAX_BYTES {
        return Ok(image_data.to_vec());
    }

    // 等比缩放
    let resized = if w > MAX_DIM || h > MAX_DIM {
        let ratio = (MAX_DIM as f64 / w as f64).min(MAX_DIM as f64 / h as f64);
        let new_w = ((w as f64) * ratio) as u32;
        let new_h = ((h as f64) * ratio) as u32;
        img.resize(
            new_w.max(1),
            new_h.max(1),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    // RGBA→RGB，PNG 编码（optimize 对应较高压缩级别）
    let rgb: image::DynamicImage = match resized {
        image::DynamicImage::ImageRgba8(rgba) => {
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
        }
        other => other,
    };

    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut buf,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    );
    rgb.write_with_encoder(encoder)
        .context("png re-encode failed")?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_small_image_passthrough() {
        // 1x1 红色 PNG（极小，无需压缩）
        let png = make_test_png(1, 1);
        let out = maybe_compress(&png).unwrap();
        assert_eq!(out, png);
    }

    #[test]
    fn compress_large_image_resizes() {
        let png = make_test_png(4096, 4096);
        let out = maybe_compress(&png).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert!(img.width() <= 2048 && img.height() <= 2048);
    }

    /// 生成纯色 PNG（用于测试）。
    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255u8, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }
}

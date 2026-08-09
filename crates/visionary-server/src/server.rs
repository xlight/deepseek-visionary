//! MCP stdio 服务层（对应 Python 版 `server.py::create_app`）。
//!
//! 基于 rmcp 2.2 的 `#[tool_router]` / `#[tool_handler]` 宏实现 4 个工具：
//! - `deepseek_vision`：上传图片并分析（完整 vision 流水线）
//! - `deepseek_vision_status`：鉴权与服务健康检查
//! - `deepseek_vision_login`：浏览器自动登录（任务 5.4 接线）
//! - `deepseek_vision_logout`：清除凭据（任务 5.5 接线）

use crate::config::Config;
use crate::hif::HifAuth;
use crate::pipeline::{self, VisionRequest};
use crate::session::SessionStore;
use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::{Deserialize, Serialize};

/// `deepseek_vision` 工具参数（对应 Python list_tools 的 inputSchema）。
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct VisionArgs {
    /// 本地图片路径（jpg / png 等），或 base64 / data URI。
    pub image: String,
    /// 对图片的问题（默认：请详细描述这张图片中的内容）。
    #[serde(default = "default_prompt")]
    pub prompt: String,
    /// 启用 DeepThink 深度思考。
    #[serde(default)]
    pub thinking: bool,
    /// 续聊：复用上一次会话并链式追问，可对比多张图片。
    #[serde(default)]
    pub continue_conversation: bool,
    /// 显式复用指定 session_id（优先于 continue_conversation）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

fn default_prompt() -> String {
    "请详细描述这张图片中的内容".into()
}

/// MCP 服务实例（持有全部共享状态）。
pub struct VisionaryServer {
    #[allow(dead_code)] // rmcp #[tool_router] 宏要求保留该字段
    tool_router: ToolRouter<Self>,
    config: Config,
    hif: HifAuth,
    session_store: SessionStore,
}

impl VisionaryServer {
    pub fn new(config: Config) -> Self {
        Self {
            tool_router: Self::tool_router(),
            hif: HifAuth::new(config.clone()),
            session_store: SessionStore::new(),
            config,
        }
    }

    /// 读取图片：支持本地路径、base64、data URI（对应 server.py 的读取逻辑）。
    fn read_image(args: &VisionArgs) -> Result<Vec<u8>, String> {
        let image_path = &args.image;
        if image_path.starts_with("data:") || is_base64(image_path) {
            let encoded = image_path
                .split_once(',')
                .map(|(_, e)| e)
                .unwrap_or(image_path);
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| format!("Failed to decode base64 image: {e}"))
        } else {
            std::fs::read(image_path)
                .map_err(|e| format!("Failed to read image `{image_path}`: {e}"))
        }
    }
}

#[tool_router]
impl VisionaryServer {
    /// 上传一张图片，用 DeepSeek 网页版的原生多模态模型分析（支持照片、
    /// 截图、带图文档）。可开启续聊以对比多张图片。
    #[tool(
        description = "Upload an image and analyze it using DeepSeek's vision model. Supports photos, screenshots, documents with images. Args: image (required, local path or base64), prompt, thinking, continue_conversation, session_id"
    )]
    async fn deepseek_vision(
        &self,
        Parameters(args): Parameters<VisionArgs>,
    ) -> Result<CallToolResult, McpError> {
        if !self.config.is_authenticated() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "DeepSeek token not configured.\n\n\
                 运行 `deepseek_vision_login` 自动登录，或手动配置：\n\
                 1. 打开 chat.deepseek.com 并登录\n\
                 2. DevTools → Application → Local Storage → userToken\n\
                 3. 将 JSON.parse(value).value 写入 ~/.deepseek-visionary/config.json 的 user_token",
            )]));
        }

        // 读取图片
        let image_data = match Self::read_image(&args) {
            Ok(d) => d,
            Err(e) => return Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
        };

        // 会话连续性（对应 Python handle_vision 的 session 解析）
        let (reuse_session_id, reuse_parent_message_id) = if let Some(sid) = &args.session_id {
            let saved = self.session_store.load();
            let parent = saved
                .filter(|s| s.session_id.as_deref() == Some(sid.as_str()))
                .and_then(|s| s.parent_message_id);
            (Some(sid.clone()), parent)
        } else if args.continue_conversation {
            let saved = self.session_store.load();
            match saved {
                Some(s) => (s.session_id, s.parent_message_id),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        match pipeline::run_vision_pipeline(
            &self.config,
            &self.hif,
            &self.session_store,
            VisionRequest {
                image_data,
                prompt: args.prompt,
                thinking: args.thinking,
                session_id: reuse_session_id,
                parent_message_id: reuse_parent_message_id,
            },
        )
        .await
        {
            Ok(output) => {
                let mut lines = vec![output.text];
                if args.continue_conversation || args.session_id.is_some() {
                    lines.push(format!(
                        "\n---\n[会话继续中] session_id: {}",
                        output.session_id
                    ));
                } else {
                    lines.push(format!(
                        "\n---\n[session_id: {}] (可用 continue_conversation=true 继续此对话)",
                        output.session_id
                    ));
                }
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    lines.join("\n"),
                )]))
            }
            Err(e) => {
                tracing::error!("vision pipeline failed: {e:#}");
                Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Vision analysis failed: {e}"
                ))]))
            }
        }
    }

    /// 检查鉴权与服务健康状态。
    #[tool(
        description = "Check authentication and service health status. Returns whether a DeepSeek token is configured and whether it passes a live validation probe."
    )]
    async fn deepseek_vision_status(&self) -> Result<CallToolResult, McpError> {
        let creds = self.config.credentials();
        let token_valid = crate::auth::AuthManager::new(&self.config).validate();
        let mut lines = vec![
            format!("DeepSeek Vision MCP Server v{}", env!("CARGO_PKG_VERSION")),
            String::new(),
            format!(
                "- Authenticated: {} ",
                if token_valid { "✅" } else { "❌" }
            ),
            format!(
                "- Token configured: {}",
                if creds.user_token.is_empty() {
                    "No"
                } else {
                    "Yes"
                }
            ),
            format!(
                "- smidV2 cookie: {}",
                if creds.smid_v2.is_empty() {
                    "❌ (optional)"
                } else {
                    "✅"
                }
            ),
            format!("- Base URL: {}", self.config.base_url),
        ];

        if !token_valid {
            lines.extend([
                String::new(),
                "Setup:".into(),
                "  运行 `deepseek_vision_login` 自动登录，或设置 DEEPSEEK_USER_TOKEN 环境变量"
                    .into(),
            ]);
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                lines.join("\n"),
            )]));
        }

        // 真实 token 校验探针：调用流水线首个需鉴权端点 create_pow_challenge。
        // （任务 1.6 spike 确认更轻量端点后替换；401 即 token 失效）
        match crate::upload::create_pow_challenge(
            &crate::client::ApiClient::new(self.config.clone())
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            "/api/v0/chat/completion",
        )
        .await
        {
            Ok(_) => lines.push(format!(
                "- Token validation: ✅ (live probe passed at {})",
                self.config.base_url
            )),
            Err(e) => {
                lines.push(format!("- Token validation: ❌ probe failed: {e}"));
                lines.push(String::new());
                lines.push("Token 可能已失效，请运行 `deepseek_vision_login` 重新登录。".into());
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(
            lines.join("\n"),
        )]))
    }

    /// 打开浏览器自动登录并抓取凭据（任务 5.4 接线）。
    #[tool(
        description = "Open a browser window to log in to chat.deepseek.com, automatically capture the token and cookies, and save them. Returns login instructions. For manual setup, edit ~/.deepseek-visionary/config.json"
    )]
    async fn deepseek_vision_login(&self) -> Result<CallToolResult, McpError> {
        crate::login::run_login(&self.config)
            .await
            .map_err(|e| McpError::internal_error(format!("login failed: {e}"), None))
    }

    /// 清除保存的凭据（任务 5.5 接线）。
    #[tool(description = "Remove saved credentials from ~/.deepseek-visionary/config.json")]
    async fn deepseek_vision_logout(&self) -> Result<CallToolResult, McpError> {
        crate::login::run_logout(&self.config)
            .await
            .map_err(|e| McpError::internal_error(format!("logout failed: {e}"), None))
    }
}

#[tool_handler]
impl ServerHandler for VisionaryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("DeepSeek 网页版原生多模态模型的 MCP 服务：上传图片分析、自动登录")
    }
}

/// base64 探测（对应 Python `_is_base64`：长度 >100 且可解码）。
fn is_base64(s: &str) -> bool {
    if s.len() <= 100 {
        return false;
    }
    base64::engine::general_purpose::STANDARD.decode(s).is_ok()
}

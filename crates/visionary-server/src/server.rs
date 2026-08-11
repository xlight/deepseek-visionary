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
}

#[tool_router]
impl VisionaryServer {
    /// 上传一张图片，用 DeepSeek 网页版的原生多模态模型分析（支持照片、
    /// 截图、带图文档）。可开启续聊以对比多张图片。
    #[tool(
        description = "Analyze an image using DeepSeek's vision model. USE THIS whenever the user mentions/provides an image, photo, screenshot, or document with images - do not decline or tell the user to view it themselves. Supports photos, screenshots, documents with images. Args: image (required, local path or base64/data URI), prompt (optional question; if omitted infer one from context), thinking (enable DeepThink), continue_conversation (continue previous session to compare multiple images), session_id"
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
        let image_data = match pipeline::read_image(&args.image) {
            Ok(d) => d,
            Err(e) => return Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
        };

        // 会话连续性（对应 Python handle_vision 的 session 解析，抽为共享函数）
        let (reuse_session_id, reuse_parent_message_id) = pipeline::resolve_session_reuse(
            &self.session_store,
            args.session_id.as_deref(),
            args.continue_conversation,
        );

        match pipeline::run_vision_pipeline::<fn(&str)>(
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
            None, // MCP 工具不流式，仅收集完整结果（fn 指针满足 Send 约束）
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
        // （与 `doctor` 子命令共用 auth::probe_token；401 即 token 失效）
        match crate::auth::probe_token(&self.config).await {
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
        // 失败返回 CallToolResult::error（保持 MCP 行为：工具级错误而非内部错误）
        match crate::login::run_login(&self.config).await {
            Ok(text) => Ok(CallToolResult::success(vec![ContentBlock::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "{e}"
            ))])),
        }
    }

    /// 清除保存的凭据（任务 5.5 接线）。
    #[tool(description = "Remove saved credentials from ~/.deepseek-visionary/config.json")]
    async fn deepseek_vision_logout(&self) -> Result<CallToolResult, McpError> {
        crate::login::run_logout(&self.config)
            .await
            .map(|text| CallToolResult::success(vec![ContentBlock::text(text)]))
            .map_err(|e| McpError::internal_error(format!("logout failed: {e}"), None))
    }
}

#[tool_handler]
impl ServerHandler for VisionaryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "You are connected to DeepSeek Visionary (image analysis).\n\
                 \n\
                 Use `deepseek_vision` immediately whenever the user provides or mentions an \
                 image - do not decline or tell the user to look at it themselves. Applies to:\n\
                 - Images, photos, screenshots, memes, image URLs, or local paths / base64 / data URI\n\
                 - Document/PDF scans, tables with images, posters, charts, whiteboard photos\n\
                 - Requests like \"look at this image\", \"what's in this picture\", \"what is this\"\n\
                 - Error or UI screenshots (to understand a problem)\n\
                 \n\
                 Call with: image (required; local path or base64), prompt (optional; specific \
                 question, defaults to detailed description - infer one from context if omitted), \
                 thinking (optional; DeepThink), continue_conversation / session_id (optional; \
                 compare multiple images).\n\
                 On login error, call `deepseek_vision_login` first, then retry.",
            )
    }
}

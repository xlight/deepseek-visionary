//! MCP stdio 服务层（对应 Python 版 `server.py::create_app`）。
//!
//! 基于 rmcp 2.2 的 `#[tool_router]` / `#[tool_handler]` 宏实现 6 个工具：
//! - `deepseek_vision`：上传图片并分析（完整 vision 流水线）
//! - `deepseek_ocr`：上传图片并提取文字（OCR 管道，等价 `visionary-server ocr`）
//! - `deepseek_vision_status`：鉴权与服务健康检查
//! - `deepseek_vision_login`：浏览器自动登录（池模式下为「添加一个账号」）
//! - `deepseek_vision_logout`：清除凭据（任务 5.5 接线）
//! - `deepseek_vision_accounts`：账号池管理（list / refresh / remove / pin）

use crate::config::Config;
use crate::hif::HifAuth;
use crate::pipeline::{self, VisionRequest};
use crate::session::SessionStore;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// `deepseek_vision` 工具参数（对应 Python list_tools 的 inputSchema）。
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct VisionArgs {
    /// 单张图片（向后兼容，旧客户端仍可用）；多图请用 `images`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// 多张图片（推荐）：本地路径（jpg / png 等）、base64 或 data URI 的数组，
    /// 一次上传由模型联合分析（与网页端多图行为一致）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
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

/// `deepseek_vision_accounts` 工具的参数面。
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AccountsToolArgs {
    /// 动作：list / refresh / remove / pin。
    pub action: String,
    /// 账号 id（remove / pin 必填）。
    #[serde(default)]
    pub account_id: Option<String>,
    /// pin 动作：true 固定该账号，false 取消固定。
    #[serde(default)]
    pub pinned: Option<bool>,
}

fn default_prompt() -> String {
    "请详细描述这张图片中的内容".into()
}

/// MCP 账号管理动作 → CLI 子命令参数（纯函数，便于单测断言 argv）。
///
/// 账号标识走**位置参数**（CLI 没有 `--account-id` 参数，用了会失败）。
/// `add` / `migrate` 刻意**不暴露**：迁移是升级期一次性动作，CLI 足够。
pub fn accounts_cli_argv(
    action: &str,
    account_id: Option<&str>,
    pinned: Option<bool>,
) -> Result<Vec<String>, McpError> {
    let action = action.trim().to_lowercase();
    let id = account_id.map(str::trim).filter(|v| !v.is_empty());
    let argv = match action.as_str() {
        "list" => vec![
            "accounts".to_string(),
            "list".to_string(),
            "--json".to_string(),
        ],
        "refresh" => vec![
            "accounts".to_string(),
            "status".to_string(),
            "--json".to_string(),
        ],
        "remove" => {
            let Some(id) = id else {
                return Err(McpError::invalid_params(
                    "action=remove requires account_id",
                    None,
                ));
            };
            vec![
                "accounts".to_string(),
                "remove".to_string(),
                "--json".to_string(),
                id.to_string(),
            ]
        }
        "pin" => match (id, pinned.unwrap_or(true)) {
            (Some(id), true) => vec![
                "accounts".to_string(),
                "use".to_string(),
                "--json".to_string(),
                id.to_string(),
            ],
            (Some(_), false) => vec![
                "accounts".to_string(),
                "use".to_string(),
                "--json".to_string(),
                "--clear".to_string(),
            ],
            (None, _) => {
                return Err(McpError::invalid_params(
                    "action=pin requires account_id",
                    None,
                ));
            }
        },
        "add" | "migrate" => {
            return Err(McpError::invalid_params(
                format!(
                    "action={action} is not exposed by this tool — use the CLI instead: visionary-server accounts {action}"
                ),
                None
            ));
        }
        other => {
            return Err(McpError::invalid_params(
                format!("unknown action: {other} (expected list | refresh | remove | pin)"),
                None,
            ));
        }
    };
    Ok(argv)
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
            // 旧格式（平铺）会话的归属账号 = 启用池时当前使用的账号。
            session_store: SessionStore::with_legacy_owner(pipeline::legacy_session_owner(&config)),
            config,
        }
    }

    /// 重载账号池：MCP 账号管理操作经 CLI 子进程落地，改的是磁盘上的池文件；
    /// 服务端内存里那份是启动时快照——不重载的话「刚加的账号没生效」。
    fn reload_account_pool(&self) {
        if let Ok(pool) = crate::accounts::load_pool() {
            if let Ok(mut guard) = self.config.pool.write() {
                *guard = pool;
            }
        }
    }
}

#[tool_router]
impl VisionaryServer {
    /// 上传一张或多张图片，用 DeepSeek 网页版的原生多模态模型联合分析
    /// （支持照片、截图、带图文档）。可开启续聊以对比多张图片。
    #[tool(
        description = "Analyze one or more images using DeepSeek's vision model. USE THIS whenever the user mentions/provides an image, photo, screenshot, or document with images - do not decline or tell the user to view it themselves. Supports photos, screenshots, documents with images. Args: images (required, array of local paths or base64/data URI), prompt (optional question; if omitted infer one from context), thinking (enable DeepThink), continue_conversation (continue previous session to compare multiple images), session_id"
    )]
    async fn deepseek_vision(
        &self,
        Parameters(args): Parameters<VisionArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.analyze_images(args, None).await
    }

    /// 从图片中提取文字（OCR 管道，等价 `visionary-server ocr <image>`）。
    #[tool(
        description = "Extract text from one or more images using DeepSeek's OCR pipeline (upload without x-model-type, completion extracts the text verbatim). USE THIS when the user wants the text/content of an image, screenshot, or document - not an interpretation. Same session/continuation semantics as deepseek_vision. Args: images (required, array of local paths or base64/data URI), prompt (optional instruction; defaults to verbatim text extraction), thinking (enable DeepThink), continue_conversation / session_id"
    )]
    async fn deepseek_ocr(
        &self,
        Parameters(args): Parameters<VisionArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.analyze_images(args, Some(crate::config::MODEL_TYPE_OCR.to_string()))
            .await
    }

    /// `deepseek_vision` / `deepseek_ocr` 共享 handler（task 2.4）。
    ///
    /// `forced_model_type`：`None` → 跟随 `config.model_type`（默认 vision，可经
    /// settings 面板 / env / settings.json 配置）；`Some("ocr")` → 恒走 OCR 管道
    /// （`deepseek_ocr` 工具面，无 modelType 参数面）。
    async fn analyze_images(
        &self,
        args: VisionArgs,
        forced_model_type: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        // 准入判定：账号池模式以「池中有无可用账号」为准（兼容配置可为空）；
        // 单账号模式维持既有 token 判定。
        if self.config.pool_mode() {
            if self.config.pool().usable_count() == 0 {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    "No usable account in the account pool.\n\n\
                     Run `deepseek_vision_login` to add an account, or check the pool with the CLI: visionary-server accounts list",
                )]));
            }
        } else if !self.config.is_authenticated() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "DeepSeek token not configured.\n\n\
                 Run `deepseek_vision_login` to auto-login, or configure manually:\n\
                 1. Open chat.deepseek.com and sign in\n\
                 2. DevTools -> Application -> Local Storage -> userToken\n\
                 3. Write JSON.parse(value).value into the user_token field of ~/.deepseek-visionary/config.json",
            )]));
        }

        // 合并 image（单张，向后兼容）与 images（多图）输入
        let image_args: Vec<String> = match (&args.images, &args.image) {
            (Some(imgs), None) if !imgs.is_empty() => imgs.clone(),
            (None, Some(img)) => vec![img.clone()],
            (Some(imgs), Some(img)) => {
                let mut all = vec![img.clone()];
                all.extend(imgs.iter().cloned());
                all
            }
            _ => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    "deepseek_vision / deepseek_ocr: at least one image is required (`image` or `images`)",
                )]))
            }
        };

        // 读取图片
        let mut images_data = Vec::with_capacity(image_args.len());
        for image in &image_args {
            match pipeline::read_image(image) {
                Ok(d) => images_data.push(d),
                Err(e) => return Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
            }
        }

        // 会话连续性：由流水线在**选号之后**统一解析（会话绑定账号，换号即断链）。

        // modelType：forced（ocr 工具恒 ocr）> config.model_type（MCP stdio 同样可配置）
        let model_type = forced_model_type.or_else(|| self.config.model_type.clone());

        match pipeline::run_vision_pipeline::<fn(&str)>(
            &self.config,
            &self.hif,
            &self.session_store,
            VisionRequest {
                images_data,
                prompt: args.prompt,
                thinking: args.thinking,
                session_id: args.session_id.clone(),
                parent_message_id: None,
                model_type,
            },
            args.continue_conversation,
            None, // MCP 工具不流式，仅收集完整结果（fn 指针满足 Send 约束）
        )
        .await
        {
            Ok(output) => {
                let mut lines = vec![output.text];
                if args.continue_conversation || args.session_id.is_some() {
                    lines.push(format!(
                        "\n---\n[conversation continuing] session_id: {}",
                        output.session_id
                    ));
                } else {
                    lines.push(format!(
                        "\n---\n[session_id: {}] (set continue_conversation=true to keep chatting)",
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
        let pool_mode = self.config.pool_mode();
        let pool = self.config.pool();
        // 凭据字段口径与 CLI `status` 一致：池模式描述**当前使用账号**。
        let creds = match pool.current_account_id() {
            Some(id) => self.config.credentials_for(&id),
            None => self.config.credentials(),
        };
        let token_valid = crate::auth::AuthManager::new(&self.config).validate();
        let usable = pool.usable_count();
        // 池模式：「已登录」= 池中有可用账号；单账号模式 = token 已配置。
        let authenticated = if pool_mode { usable > 0 } else { token_valid };
        let mut lines = vec![
            format!("DeepSeek Vision MCP Server v{}", env!("CARGO_PKG_VERSION")),
            String::new(),
            format!(
                "- Authenticated: {} ",
                if authenticated { "[OK]" } else { "[FAIL]" }
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
                    "[FAIL] (optional)"
                } else {
                    "[OK]"
                }
            ),
            format!("- Base URL: {}", self.config.base_url),
        ];

        if pool_mode {
            let view = pool.view(!crate::cli::ignored_env_vars(&self.config).is_empty());
            lines.push(format!(
                "- Account pool: {} account(s) — healthy {}, muted {}, expired {}",
                view.total, view.healthy, view.muted, view.expired
            ));
            if let Some(current) = &view.current {
                lines.push(format!("- Current account: {current}"));
            }
            if let Some(pinned) = &view.pinned {
                lines.push(format!("- Pinned account: {pinned}"));
            }
            if view.env_ignored {
                lines.push(
                    "- Environment ignored (pool mode): DEEPSEEK_USER_TOKEN — rotation uses the account pool only."
                        .into(),
                );
            }
        }

        if !authenticated {
            lines.extend([String::new(), "Setup:".into()]);
            if pool_mode {
                // 池模式下环境变量被忽略，SHALL NOT 引导用户去设置它。
                lines.push(
                    "  Run `deepseek_vision_login` to add an account, or inspect the pool: visionary-server accounts list"
                        .into(),
                );
            } else {
                lines.push(
                    "  Run `deepseek_vision_login` to auto-login, or set the DEEPSEEK_USER_TOKEN environment variable"
                        .into(),
                );
            }
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                lines.join("\n"),
            )]));
        }

        // 只读探针（GET /api/v0/users/current）：与 CLI `status` / `doctor` 同一判定语义，
        // 纯 GET、无 PoW、无配额消耗。
        let probe = match pool.current_account_id() {
            Some(id) => {
                crate::account_health::probe_account_id(&self.config, &id, Duration::from_secs(15))
                    .await
            }
            None => {
                crate::account_health::probe_config(&self.config, Duration::from_secs(15)).await
            }
        };
        match probe {
            Ok(health) => {
                lines.push(format!(
                    "- Token validation: [OK] (read-only probe passed at {})",
                    self.config.base_url
                ));
                if health.muted {
                    let until = health
                        .mute_until_utc()
                        .unwrap_or_else(|| "unknown".to_string());
                    lines.push(format!(
                        "- Account muted: [WARN] uploads are muted until {until}"
                    ));
                }
            }
            Err(e) => {
                lines.push(format!("- Token validation: [FAIL] probe failed: {e}"));
                lines.push(String::new());
                lines.push(
                    "The token may have expired. Run `deepseek_vision_login` to log in again."
                        .into(),
                );
            }
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(
            lines.join("\n"),
        )]))
    }

    /// 打开浏览器自动登录并抓取凭据（任务 5.4 接线）。
    #[tool(
        description = "Open a browser window to log in to chat.deepseek.com, automatically capture the token and cookies, and save them. When the account pool is enabled this ADDS an account (it does not replace the current one). Returns login instructions. For manual setup, edit ~/.deepseek-visionary/config.json"
    )]
    async fn deepseek_vision_login(&self) -> Result<CallToolResult, McpError> {
        // 登录在**后台**进行：账号被静音时把「换号指引」即时返回给调用方，
        // 同时让浏览器流程继续等待用户在同一窗口换号——换号成功后后台流程
        // 自动保存凭据（调用方可用状态工具查询进度）。
        let config = self.config.clone();
        // 池模式下 login = **添加一个账号**（不是替换当前账号）：既有 run_login
        // 在已配置 token 时直接返回「Token already configured」、不打开浏览器，
        // 迁移后 config.json 仍有凭据 → 用户将永远无法经 MCP 添加第二个账号。
        let pool_mode = config.pool_mode();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::login::LoginEvent>();
        let mut handle = tokio::spawn(async move {
            if pool_mode {
                crate::login::run_login_add_account_with_events(&config, tx).await
            } else {
                crate::login::run_login_with_events(&config, tx).await
            }
        });

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(crate::login::LoginEvent::Muted {
                            account_id,
                            label,
                            mute_until,
                        }) => {
                            let guidance = crate::login::muted_guidance(
                                &label, &account_id, mute_until,
                            );
                            // 后台任务继续等待换号；句柄随本函数返回被丢弃，
                            // 但 tokio 任务**不会**因此被取消。
                            return Ok(CallToolResult::success(vec![ContentBlock::text(
                                guidance,
                            )]));
                        }
                        None => {}
                    }
                }
                result = &mut handle => {
                    return match result {
                        Ok(Ok(text)) => {
                            Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
                        }
                        Ok(Err(e)) => Ok(CallToolResult::error(vec![ContentBlock::text(
                            format!("{e}"),
                        )])),
                        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
                            format!("login task failed: {e}"),
                        )])),
                    };
                }
            }
        }
    }

    /// 账号池管理：列出 / 刷新健康 / 移除 / 固定账号。
    #[tool(
        description = "Manage the DeepSeek account pool. Args: action (list | refresh | remove | pin), account_id (required for remove/pin), pinned (for pin: true pins the account, false unpins it). 'add' and 'migrate' are intentionally not exposed by this tool."
    )]
    async fn deepseek_vision_accounts(
        &self,
        Parameters(args): Parameters<AccountsToolArgs>,
    ) -> Result<CallToolResult, McpError> {
        let action = args.action.trim().to_lowercase();
        // 动作 → CLI 子命令（账号标识走**位置参数**；CLI 没有 --account-id 参数）。
        let argv = accounts_cli_argv(&action, args.account_id.as_deref(), args.pinned)?;

        // 经 CLI 子进程落地（与 CLI / 插件同源，避免三端语义漂移）。
        let exe = std::env::current_exe().map_err(|e| {
            McpError::internal_error(format!("failed to locate the server binary: {e}"), None)
        })?;
        let output = tokio::process::Command::new(exe)
            .args(&argv)
            .output()
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to spawn the accounts CLI: {e}"), None)
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // 以 JSON 输出为权威结果（SHALL NOT 仅凭退出码判定成败）。
        let parsed: Option<serde_json::Value> = serde_json::from_str(stdout.trim()).ok();
        if !output.status.success() {
            let message = parsed
                .as_ref()
                .and_then(|v| v.get("error"))
                .and_then(|e| e.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if stderr.trim().is_empty() {
                        format!("accounts {action} failed")
                    } else {
                        stderr.trim().to_string()
                    }
                });
            return Err(McpError::internal_error(message, None));
        }
        // 重载账号池：子进程改的是磁盘上的池文件，服务端内存里那份是启动时快照。
        self.reload_account_pool();
        let payload = parsed
            .map(|v| v.to_string())
            .unwrap_or_else(|| stdout.clone());
        Ok(CallToolResult::success(vec![ContentBlock::text(payload)]))
    }

    /// 清除保存的凭据（任务 5.5 接线）。
    #[tool(
        description = "Clear the saved single-account credentials. When the account pool is enabled this does NOT remove pooled accounts and does NOT stop rotation — use deepseek_vision_accounts (action=remove) to drop a specific account."
    )]
    async fn deepseek_vision_logout(&self) -> Result<CallToolResult, McpError> {
        crate::login::run_logout(&self.config)
            .await
            .map(|text| CallToolResult::success(vec![ContentBlock::text(text)]))
            .map_err(|e| McpError::internal_error(format!("logout failed: {e}"), None))
    }
}

#[tool_handler]
impl ServerHandler for VisionaryServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
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
                 \n\
                 Use `deepseek_ocr` when the user wants the raw text extracted from an image \
                 (text extraction, not interpretation), e.g. screenshots of documents or code.\n\
                 Use `deepseek_vision_accounts` (action: list | refresh | remove | pin) to manage the account pool; account ids are passed positionally by the CLI it spawns.\n\
                 On login error, call `deepseek_vision_login` first, then retry.",
            )
    }
}
#[cfg(test)]
mod accounts_tool_tests {
    use super::*;

    #[test]
    fn action_maps_to_cli_argv() {
        assert_eq!(
            accounts_cli_argv("list", None, None).unwrap(),
            vec!["accounts", "list", "--json"]
        );
        assert_eq!(
            accounts_cli_argv("refresh", None, None).unwrap(),
            vec!["accounts", "status", "--json"]
        );
        assert_eq!(
            accounts_cli_argv("remove", Some("acc-1"), None).unwrap(),
            vec!["accounts", "remove", "--json", "acc-1"]
        );
        assert_eq!(
            accounts_cli_argv("pin", Some("acc-1"), Some(true)).unwrap(),
            vec!["accounts", "use", "--json", "acc-1"]
        );
        assert_eq!(
            accounts_cli_argv("pin", Some("acc-1"), Some(false)).unwrap(),
            vec!["accounts", "use", "--json", "--clear"]
        );
    }

    #[test]
    fn add_and_migrate_are_not_exposed() {
        assert!(accounts_cli_argv("add", None, None).is_err());
        assert!(accounts_cli_argv("migrate", None, None).is_err());
    }

    #[test]
    fn missing_account_id_is_rejected() {
        assert!(accounts_cli_argv("remove", None, None).is_err());
        assert!(accounts_cli_argv("remove", Some("  "), None).is_err());
        assert!(accounts_cli_argv("pin", None, None).is_err());
        assert!(accounts_cli_argv("bogus", None, None).is_err());
    }
}

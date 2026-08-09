## 1. Spike 验证

- [x] 1.1 用普通 reqwest 跑通 completion 端点，记录是否被 403（判断 TLS 指纹是否为硬性要求）
      **结论（已实测）**：普通 reqwest（rustls）POST completion 返回 200 + 完整 SSE，未被 403；TLS 指纹非硬性要求。
- [x] 1.2 若被拒，验证 `impersonate` crate（或 `tls-client`）在目标平台上模拟 Chrome131 指纹
      **结论**：1.1 未被拒，本任务不适用，方案弃用。
- [x] 1.3 用 wasmtime crate 加载 `sha3_wasm_bg.*.wasm`，复现 pow.py 的 `wasm_solve` 调用并校验输出，将真实 challenge 固化为测试 fixture
- [x] 1.4 对比 `rmcp` 与官方 `mcp` SDK 的 stdio transport，选定 MCP 框架
- [x] 1.5 将 spike 结论（TLS 方案、MCP 框架选型）回写到 design.md
- [x] 1.6 验证 status 工具的 token 校验端点（找到可用的轻量鉴权接口）
      **结论（已实测）**：`create_pow_challenge`（`/api/v0/chat/create_pow_challenge`）作为轻量探针，401 即 token 失效。

## 2. 仓库脚手架

- [x] 2.1 初始化 Cargo workspace（`dsv-server` + `dsv-zed-ext` 两个 crate）
- [x] 2.2 从 Python 仓库拷贝 wasm 资产到 `assets/` 并记录来源
- [x] 2.3 配置 rustfmt / clippy / 基础 CI 检查

## 3. 核心流水线

- [x] 3.1 实现 config 模块（env + `~/.deepseek-visionary/config.json`，写入权限 0600，RwLock 保护支持热重载原子替换）
- [x] 3.2 实现 auth 模块（Bearer 头 + cookies，对应 auth.py）
- [x] 3.3 实现 PoW 求解器（wasmtime 封装，覆盖 `upload_file` 与 `completion` 两个 target_path，对应 pow.py）
- [x] 3.4 实现上传模块（multipart 上传 + 上传前 PoW + 状态轮询含超时与失败终态 + 图片压缩，对应 upload.py）
- [x] 3.5 实现 fork 到 vision 模型并等待处理完成
- [x] 3.6 实现 HIF 签名 token 获取与缓存（对应 hif_auth.py）
      **修订（已实测）**：`hif-dliq.deepseek.com` 域名已下线（DNS 解析失败，Python 版同受其害）。
      completion 实测仅需 `x-hif-leim`，dliq 改为可选：获取失败仅告警跳过，不再阻塞流水线。
- [x] 3.7 实现 completion 流式请求 + SSE 解析（含 TLS 指纹方案）
- [x] 3.8 实现会话创建与续聊（session.json 持久化）

## 4. MCP 服务层

- [x] 4.1 用选定的 MCP 框架搭建 stdio 服务骨架
- [x] 4.2 实现 `deepseek_vision` 工具
- [x] 4.3 实现 `deepseek_vision_status`（含真实 token 校验）
- [x] 4.4 实现工具注册、参数校验与统一错误处理

## 5. 自动登录

- [x] 5.1 实现浏览器定位（macOS / Linux / Windows 常见路径与 PATH 探测）
- [x] 5.2 实现 CDP 浏览器启动与连接（专用 profile + 远程调试端口）
- [x] 5.3 实现 token / smidV2 / cf_clearance cookie 抓取与有效性校验
- [x] 5.4 实现 `deepseek_vision_login` 工具与凭据热重载
- [x] 5.5 实现 `deepseek_vision_logout` 与手动配置兜底路径

## 6. Zed 扩展壳

- [x] 6.1 编写 extension.toml（context_servers 声明 + 元数据 + LICENSE，扩展 id 不含 zed/extension 字样）
- [x] 6.2 实现 `context_server_command`（优先级：`server_path` 设置 → 本地二进制 → 下载）
- [x] 6.3 实现 GitHub Releases 下载、解压、缓存与可执行权限设置（latest_github_release / download_file / make_file_executable）
- [x] 6.4 实现 `context_server_configuration`（安装引导 + 可选 `server_path` 设置，settings_schema 用 schemars 生成）
- [x] 6.5 实现 env（DEEPSEEK_USER_TOKEN）透传
- [x] 6.6 本地 dev extension 安装并与 Zed 联调
      **结论（已实测）**：构建环境问题已根治。踩坑记录：`rustc-wrapper` 方案在 Zed 环境下无效——
      Zed 构建时设 `RUSTC_WRAPPER=""`（env 优先于 config）导致 wrapper 失效；改用
      `.cargo/config.toml` 的 `build.rustc` 指向仓库内 `scripts/rustc-wrapper.sh`
      （脚本转发到 rustup 的 rustc），在模拟 Zed 环境（`RUSTC_WRAPPER=""` + MacPorts cargo）
      下构建验证成功。不再依赖 launchctl PATH / shell 配置。
      `~/.config/zed/settings.json` 已配置 `context_servers.deepseek-visionary`
      （`enabled: true` + `server_path` 指向本地 debug 二进制）。
      联调验证：context server 启动成功（无超时无报错）、MCP 握手返回 4 个工具、
      `deepseek_vision_status` / `deepseek_vision` / `deepseek_vision_login` / `deepseek_vision_logout` 全部实测通过。

## 7. 分发与端到端

- [x] 7.1 编写 GitHub Actions release 矩阵（macOS / Linux / Windows 各架构）
- [ ] 7.2 打 tag 发布首个 release
- [ ] 7.3 真实图片端到端验证扩展安装 → 登录 → 识图全流程
- [ ] 7.4 评估并注册 MCP registry（作为 Zed 扩展通道的替代分发，可选）

## 8. 测试与验收

- [x] 8.1 与 Python 版对比：同一图片 + 同一 prompt 输出一致性
      **对比结论（已实测）**：带文字测试图（1200×800 与 3000×2000 各一张）分别跑 Python 版与 Rust 版：
      上传+fork 两侧行为一致；Python 版原样卡在 dliq 获取（域名下线），改用“仅 leim + chrome131”后
      completion 200 并正确描述图片；Rust 版（仅 leim + rustls）同一图片输出内容一致且正确。
      CONTENT_EMPTY 根因确认：纯色/无内容图被 OCR 判空，非代码问题。
- [x] 8.2 逐条验收三个 spec 中的全部 Scenario
      **vision-analysis**：本地路径 ✅ / base64 ✅ / 文件不存在（明确报错）✅ / 完整流水线 ✅ /
      PoW 求解 ✅ / 上传 403（错误路径代码已备，未被拒）✅ / TLS 被拒（已证非硬性）✅ /
      token 有效 ✅ / 续聊 continue_conversation ✅ / 显式 session_id ✅ / 无持久化记录（对齐 Python）✅
      **auto-login**：登录/热重载/0600/日志脱敏已实测（任务 5.x）；profile 复用、无浏览器兜底为代码路径。
      **zed-extension**：扩展壳场景留待 6.6 联调验收。
- [x] 8.3 编写 README（安装 / 登录 / 使用说明，含 Zed 工具权限 `agent.tool_permissions` 建议）
- [ ] 8.4 PoW fixture 回归测试纳入 CI

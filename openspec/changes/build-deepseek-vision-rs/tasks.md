## 1. Spike 验证

- [ ] 1.1 用普通 reqwest 跑通 completion 端点，记录是否被 403（判断 TLS 指纹是否为硬性要求）
- [ ] 1.2 若被拒，验证 `impersonate` crate（或 `tls-client`）在目标平台上模拟 Chrome131 指纹
- [x] 1.3 用 wasmtime crate 加载 `sha3_wasm_bg.*.wasm`，复现 pow.py 的 `wasm_solve` 调用并校验输出，将真实 challenge 固化为测试 fixture
- [x] 1.4 对比 `rmcp` 与官方 `mcp` SDK 的 stdio transport，选定 MCP 框架
- [ ] 1.5 将 spike 结论（TLS 方案、MCP 框架选型）回写到 design.md
- [ ] 1.6 验证 status 工具的 token 校验端点（找到可用的轻量鉴权接口）

## 2. 仓库脚手架

- [x] 2.1 初始化 Cargo workspace（`dsv-server` + `dsv-zed-ext` 两个 crate）
- [x] 2.2 从 Python 仓库拷贝 wasm 资产到 `assets/` 并记录来源
- [x] 2.3 配置 rustfmt / clippy / 基础 CI 检查

## 3. 核心流水线

- [x] 3.1 实现 config 模块（env + `~/.deepseek-vision/config.json`，写入权限 0600，RwLock 保护支持热重载原子替换）
- [x] 3.2 实现 auth 模块（Bearer 头 + cookies，对应 auth.py）
- [x] 3.3 实现 PoW 求解器（wasmtime 封装，覆盖 `upload_file` 与 `completion` 两个 target_path，对应 pow.py）
- [x] 3.4 实现上传模块（multipart 上传 + 上传前 PoW + 状态轮询含超时与失败终态 + 图片压缩，对应 upload.py）
- [x] 3.5 实现 fork 到 vision 模型并等待处理完成
- [x] 3.6 实现 HIF 签名 token 获取与缓存（对应 hif_auth.py，含 dliq DNS 兜底）
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
- [ ] 6.6 本地 dev extension 安装并与 Zed 联调

## 7. 分发与端到端

- [x] 7.1 编写 GitHub Actions release 矩阵（macOS / Linux / Windows 各架构）
- [ ] 7.2 打 tag 发布首个 release
- [ ] 7.3 真实图片端到端验证扩展安装 → 登录 → 识图全流程
- [ ] 7.4 评估并注册 MCP registry（作为 Zed 扩展通道的替代分发，可选）

## 8. 测试与验收

- [ ] 8.1 与 Python 版对比：同一图片 + 同一 prompt 输出一致性
- [ ] 8.2 逐条验收三个 spec 中的全部 Scenario
- [x] 8.3 编写 README（安装 / 登录 / 使用说明，含 Zed 工具权限 `agent.tool_permissions` 建议）
- [ ] 8.4 PoW fixture 回归测试纳入 CI

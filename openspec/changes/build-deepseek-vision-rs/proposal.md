## Why

现有 Python 版 `deepseek-vision-mcp` 有两个主要问题：

1. **上手门槛高**：依赖 Python 3.11+ 与多个重量级依赖（`curl-cffi`、`wasmtime`、`Pillow` 等），且需要用户手动从浏览器 DevTools 复制 `userToken` 再配置环境变量，体验极差。
2. **分发形态不匹配**：Zed 的 MCP 扩展生态面向"二进制或 npm 包"分发，Python 服务不适合；且 Zed 正在将 MCP 服务器转向官方 MCP registry，独立原生二进制可同时覆盖扩展市场与 registry 两条分发通道。

目标：用 Rust 全量重写为单一原生二进制，内置浏览器自动登录获取 token，并封装为 Zed MCP 扩展。

## Capabilities

### New Capabilities
- `vision-analysis` — 图像分析能力：上传 → fork → PoW → HIF → completion 完整流水线，以及会话续聊、状态检查等 MCP 工具
- `auto-login` — 自动登录能力：通过 CDP 控制浏览器读取 localStorage token 并写入本地配置，无需手动复制
- `zed-extension` — Zed 扩展壳：`context_server_command` 下载/定位原生二进制并启动 MCP 服务

### Modified Capabilities
（无 — 全新仓库，无既有 spec）

## Impact

- **新仓库** `deepseek-vision-rs`（Cargo workspace，含 `dsv-server` 原生二进制与 `dsv-zed-ext` 扩展壳两个 crate）
- **复用资产**：`sha3_wasm_bg.7b9ca65ddd.wasm`（PoW 求解器，从 Python 仓库拷贝）
- **主要依赖**：`rmcp`（或官方 `mcp` SDK，spike 时定）、`reqwest`、`wasmtime`、`chromiumoxide`（CDP）、`image`、`zed_extension_api`
- **技术风险**：completion 端点的 TLS 指纹校验（Python 版用 curl_cffi 模拟 Chrome131）——需 spike 验证 Rust 侧方案
- **平台**：macOS / Linux / Windows 多平台二进制，GitHub Actions 发布

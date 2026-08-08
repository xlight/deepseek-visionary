# 资产来源

## `sha3_wasm_bg.7b9ca65ddd.wasm`

- **来源**: DeepSeek 网页端（chat.deepseek.com）下发的 PoW 哈希器 wasm 模块
- **出处**: 从 `deepseek-vision-mcp` Python 仓库拷贝
  - 原始路径: `src/deepseek_vision_mcp/wasm/sha3_wasm_bg.7b9ca65ddd.wasm`
  - 仓库: https://github.com/qqshrimp/deepseek-vision-mcp （本地路径 `/Users/xlight/Projects/deepseek-vision-mcp`）
- **用途**: `visionary-server` 通过 `wasmtime` 加载并调用导出函数 `wasm_solve` 求解 PoW challenge
- **大小**: 26,612 字节
- **哈希**: 随仓库 git 跟踪，可通过 `shasum -a 256` 校验
- **注意**: 该资产为 DeepSeek 站内逆向产物，随仓库 MIT 分发，与 Python 版现状一致

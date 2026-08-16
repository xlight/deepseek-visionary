# windows-install-friendliness

## Why

Windows 用户在安装与使用 DeepSeek Visionary 时遇到多个摩擦点（v0.6.0 实测）：

1. **`HOME not set` 崩溃**：PowerShell 环境没有 `HOME` 环境变量（Windows 用 `USERPROFILE`），但 `crates/visionary-server/src/config.rs` 的 `data_dir()` 直接读 `HOME` 并在缺失时报错——`login` / `status` / `vision` 在 Windows 全部第一步就失败。讽刺的是 `onboarding.rs` 已有正确的跨平台 `home_dir()`（HOME → USERPROFILE 回退），`config.rs` 没有复用。
2. **npm 全局包对 DSH 插件结构性不可用**：`npm install -g @xlight-oss/visionary-server` 在 Windows 只在 PATH 生成 `.cmd` / `.ps1` shim（node 脚本包装），真实 exe 藏在包内 `node_modules/.bin_real/`；shim 层用 `spawnSync` + `stdio: "inherit"` 转发，插件直接 spawn shim 会丢失 stdout 管道且 kill 链路断裂（孤儿进程）。插件 `resolveBinaryPath` 只按 `visionary-server.exe` 扫 PATH——找不到 → "binary not found"。
3. **binary 解析时机**：插件 `apply()` 时解析一次并闭包缓存，用户改 PATH / 设 `DEEPSEEK_VISIONARY_BIN` 后必须重启 DSH 才生效。
4. **提示与文档 Unix 化**：`binaryMissingHelp` 只给 `curl | sh` / brew 命令；README 说安装到 `$HOME\.cargo\bin` 未澄清"无需安装 cargo"。

目标：Windows 安装与使用达到与 macOS/Linux 对等的顺滑度。

## What Changes

- **Rust（visionary-server）**：`config.rs` 的 `data_dir()` 改用跨平台 home 解析（`HOME` → `USERPROFILE` 回退，复用/提升 `onboarding::home_dir()`）；`cli.rs` 的 skill install 路径同样改走 `home_dir()`（当前 fallback 是当前目录 `"."`，Windows 会装错位置）。版本同步经 `scripts/bump_version.py` 统一完成。
- **DSH 插件（@xlight-oss/visionary-dsh）**：
  - `resolveBinaryPath` 在 win32 下追加 npm shim 解析：PATH 扫描找不到 `.exe` 时，扫描 `visionary-server.cmd` / `.ps1`，正则提取 shim 文本中的 `node_modules\@xlight-oss\visionary-server\run-visionary-server.js` 引用，推导包目录，再探测固定布局 `node_modules\.bin_real\visionary-server.exe`——找到即 spawn **exe 真身**（保持 pipe + kill 链路完好，不 spawn shim）
  - binary 解析从 `apply()` 改为每次工具调用懒解析（成本为几次 statSync），PATH / 环境变量改动即时生效
  - `binaryMissingHelp` 按 `process.platform` 分支：win32 给出 PowerShell / npm / binaryPath 指引
- **文档（README）**：澄清 `$HOME\.cargo\bin` 只是命名约定（不要求安装 cargo），非 Rust 用户可用 `VISIONARY_SERVER_INSTALL_DIR` 自定义目录或直接用 npm 全局包

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `visionary-server`: CLI 在 Windows（无 `HOME` 环境变量）下正常解析用户目录——`login` / `status` / `vision` / `skill install` 全部可用
- `dsh-plugin`: 二进制解析在 Windows npm 全局安装场景下自动定位 exe 真身；环境变量 / PATH 改动即时生效（无需重启 DSH）

## Impact

- **代码**：`crates/visionary-server/src/config.rs`、`crates/visionary-server/src/cli.rs`、`crates/visionary-server/src/onboarding.rs`（若提升 home_dir 位置）；`packages/dsh-plugin/lib/index.mjs`
- **测试**：Rust 侧新增 Windows 风格环境注入单测（无 `HOME`、有 `USERPROFILE`）；插件侧新增 shim 解析单测 + 懒解析单测
- **发布**：Rust 修复随 cargo-dist 发布（npm `@xlight-oss/visionary-server` 的 postinstall 下载新二进制）；插件修复随 `@xlight-oss/visionary-dsh` 发布；一次 bump（v0.6.1，bugfix）双包同发
- **文档**：`README.md` 安装节补充 Windows 澄清
- **兼容**：无破坏性变更；行为语义不变，仅修复平台缺陷

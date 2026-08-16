# Tasks: Windows 安装友好修复

## 1. Rust 跨平台 home 解析

- [x] 1.1 提升 `onboarding::home_dir()` 为公共工具（`pub(crate)` 可见），保持 HOME → USERPROFILE 回退逻辑
- [x] 1.2 `config.rs::data_dir()` 改用 `home_dir()?`，移除直接读 `HOME` + "HOME not set" 上下文
- [x] 1.3 `cli.rs` 的 skill install 路径改用 `home_dir()?`（替换 fallback 到当前目录 `"."` 的行为）
- [x] 1.4 新增单测：注入"无 HOME、有 USERPROFILE"的环境，验证 `data_dir()` / skill 路径解析正确

## 2. 插件 npm shim 解析（win32）

- [x] 2.1 `resolveBinaryPath` 增加 shim 解析：PATH 扫描 `.exe` 失败后，扫描 `visionary-server.cmd` / `.ps1`
- [x] 2.2 正则提取 shim 中的 `node_modules\@xlight-oss\visionary-server\run-visionary-server.js` 引用 → 推导包目录 → 探测 `.bin_real\visionary-server.exe` 真身
- [x] 2.3 新增单测：注入模拟 shim 内容 + 临时目录树，验证解析出真身路径；shim 缺失 / 格式异常时回退 null

## 3. 插件懒解析

- [x] 3.1 `apply()` 移除 binary 预解析缓存，`requireBinary()` 每次调用 `resolveBinaryPath(config)`
- [x] 3.2 新增单测：解析失败 → 设 `DEEPSEEK_VISIONARY_BIN` 后再调用 → 无需重启即命中

## 4. 提示与文档

- [x] 4.1 `binaryMissingHelp()` win32 平台分支（npm 全局安装命令 + binaryPath 指引 + 重启提示）
- [x] 4.2 README 安装节澄清 `.cargo\bin` 命名约定（无需 cargo）+ `VISIONARY_SERVER_INSTALL_DIR` 自定义目录

## 5. 验证与发布

- [x] 5.1 `cargo test` 全绿（含新单测）
- [x] 5.2 `node --test` 全绿（含新单测）
- [ ] 5.3 `bump_version.py 0.6.1 --release` 发版，监控 cargo-dist / dsh-plugin / update-server-json workflows
- [ ] 5.4 发布后核对 server.json fileSha256 已回填 v0.6.1 真实哈希

# Design: Windows 安装友好修复

## D1: 跨平台 home 解析（Rust）

`onboarding.rs` 已有正确的 `home_dir()`：

```rust
pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .context("HOME/USERPROFILE not set")
}
```

`config.rs` 的 `data_dir()` 直接 `env::var_os("HOME")` + `context("HOME not set")` —— Windows 无 `HOME` 即失败。选择：**把 `home_dir()` 提升为公共工具**（放 `onboarding.rs` 并 `pub(crate)` 引用，或挪到独立 `paths` 模块），`config.rs` / `cli.rs` 全部改用它。

- `config.rs::data_dir()` → `Ok(home_dir()?.join(".deepseek-visionary"))`
- `cli.rs::skill_install` → `let home = home_dir()?`（当前 fallback 是 `PathBuf::from(".")`，Windows 会把 skill 装到当前目录——错误位置）
- 错误信息统一为 `HOME/USERPROFILE not set`

> 不引入 `dirs` crate：现有实现已足够，减少依赖面。

## D2: npm shim 解析（插件，win32）

npm 全局包的运行链路（tarball 已确认）：

```
<prefix>\visionary-server.cmd        (npm shim)
  └─ node "<prefix>\node_modules\@xlight-oss\visionary-server\run-visionary-server.js" %*
       └─ require("./binary").run("visionary-server")
            └─ spawnSync(<pkg>\node_modules\.bin_real\visionary-server.exe, args,
                         { stdio: "inherit" })
                 └─ process.exit(result.status)
```

**不能直接 spawn shim**：`stdio: "inherit"` 使插件的 stdout 管道拿不到输出；`spawnSync` + cmd→node→exe 三层让超时 kill 只杀到 cmd，exe 成为孤儿。

**解析策略**（`resolveBinaryPath` win32 分支，在 PATH 扫描 `.exe` 失败后）：

1. 扫描 PATH 目录中的 `visionary-server.cmd`（优先）与 `visionary-server.ps1`
2. 读 shim 文本，正则提取相对引用：`/node_modules[\\/]@xlight-oss[\\/]visionary-server[\\/]run-visionary-server\.js/`
3. 包目录 = `join(shimDir, 匹配到的相对路径的 dirname)`；真身 = `join(包目录, "node_modules", ".bin_real", "visionary-server.exe")`
4. 若真身存在返回其路径（spawn 真身，pipe/kill 完好）；否则继续尝试下一个候选，全部失败返回 null

> 固定布局探测优于"解析 js 再跳一层"：`.bin_real` 是 cargo-dist npm 包的稳定内部布局（`binary-install.js` 硬编码 `join(__dirname, "node_modules", ".bin_real")`），tarball 已核实。

## D3: 懒解析（插件）

现状 `apply()` 开头 `const binary = resolveBinaryPath(config)` 闭包缓存。改为：`apply` 不再解析，工具执行时（`requireBinary()`）每次调用 `resolveBinaryPath(config)`。

- 成本：每次工具调用几次 `statSync`（PATH 目录数级），可忽略
- 收益：用户设 `DEEPSEEK_VISIONARY_BIN` / 修 PATH / 装完二进制后**立即生效**，无需重启 DSH
- 版本探测（apply 时 fire-and-forget）保留：若解析失败则 versionInfo 保持 unknown，工具仍可工作（错误信息引导安装）

## D4: 平台化提示与文档

- `binaryMissingHelp()` 按 `process.platform === "win32"` 分支：
  - win32：npm 全局安装命令 + `Config.binaryPath` / `DEEPSEEK_VISIONARY_BIN` 指引 + 重启 DSH 提示
  - 其他：现有 `curl | sh` / brew / npm
- README 安装节澄清：`$HOME\.cargo\bin` 仅为 cargo-dist 默认目录名，**不要求安装 cargo**；非 Rust 用户可用 `VISIONARY_SERVER_INSTALL_DIR` 环境变量自定义目录，或直接用 npm 全局包（`npm install -g @xlight-oss/visionary-server`）

## D5: 版本与发布

- 语义：bugfix → `v0.6.1`（major.minor 不变，`COMPAT_MINOR` 仍为 `0.6`，插件与二进制无需相互升级约束）
- 单次 `bump_version.py 0.6.1 --release` 双包同发：
  - cargo-dist → GitHub Release + `@xlight-oss/visionary-server` npm 包（postinstall 下载新二进制）
  - `dsh-plugin-release.yml` → `@xlight-oss/visionary-dsh` npm 包
  - `update-server-json.yml`（workflow_run）自动回填 server.json fileSha256
- 已知：Update Zed Extension workflow 为既有失败（v0.3.0 起，非本次引入），不阻塞

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| shim 格式随 npm 版本变化 | 正则只匹配 `@xlight-oss/visionary-server` 包路径引用（npm 生成的 shim 中稳定出现）；解析失败回退现有行为（null → 友好错误） |
| USERPROFILE 也缺失（极端环境） | 报 `HOME/USERPROFILE not set`，与现状一致的明确错误 |
| 懒解析引入的每调用开销 | 仅数次 statSync，量级可忽略；无锁、无 IO 写入 |

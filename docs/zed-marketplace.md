# Zed 官方市场上架指南

将 `deepseek-visionary` 发布到 Zed 官方扩展市场（`zed: extensions` 可搜索安装）。

> 与 MCP Registry（`server.json`）是两条独立通道：Zed 列表消费的是扩展壳
> `crates/visionary-zed-ext`（wasm），原生二进制仍从 GitHub Releases 下载。

## 前置检查清单（每次发版前）

- [ ] `crates/visionary-zed-ext/extension.toml` 的 `version` 与 workspace 版本一致
      （cargo-dist 要求 `tag == v{version}`，Zed 要求 `extensions.toml` 登记的
      version 与 `extension.toml` 一致，三处需对齐）
- [ ] 扩展 ID `deepseek-visionary` 不含 `zed`/`extension` 字样，且在
      `zed-industries/extensions` 顶层 `extensions.toml` 未被占用
- [ ] `crates/visionary-zed-ext/LICENSE`（MIT，在接受的许可证列表内）存在
- [ ] 扩展只用 Zed API / std，MCP server 二进制从 GitHub Releases 下载
      （`lib.rs` 的 `latest_github_release` + `download_file`）
- [ ] 已作为 dev extension 在本地实测过（`Extensions → Install Dev Extension`）

## 首次上架（一次性）

1. fork `zed-industries/extensions`（建议 fork 到个人账号，方便 Zed staff 直接改）

   ```bash
   gh repo fork zed-industries/extensions --clone --remote=true
   cd extensions
   git submodule init
   git submodule update
   ```

2. 挂 submodule（必须 HTTPS，目录名 = 扩展 ID）：

   ```bash
   git submodule add https://github.com/xlight/deepseek-visionary.git extensions/deepseek-visionary
   ```

   要求：仓库公开、submodule 指向分支上的 commit（不能是 detached）。

3. 顶层 `extensions.toml` 登记条目（扩展在 workspace 子目录，需 `path` 字段）：

   ```toml
   [deepseek-visionary]
   submodule = "extensions/deepseek-visionary"
   path = "crates/visionary-zed-ext"
   version = "0.2.1"
   ```

4. 排序并提交 PR：

   ```bash
   pnpm install   # 首次需要
   pnpm sort-extensions
   git add -A
   git commit -m "Add deepseek-visionary context server extension"
   git push -u origin main
   gh pr create --repo zed-industries/extensions --fill
   ```

5. PR 描述要点（审核者关注）：
   - 已作为 dev extension 实测的说明
   - 二进制下载逻辑：`latest_github_release` + 按平台匹配
     `visionary-server-<arch>-<os>[.exe]` asset，`DownloadedFileType::Uncompressed`
   - 许可证：MIT（`crates/visionary-zed-ext/LICENSE`）

## 更新流程（每次发版后）

```bash
cd extensions
git submodule update --remote extensions/deepseek-visionary
# 编辑 extensions.toml，把 version 改为新版本（需与 extension.toml 一致）
pnpm sort-extensions
git add -A && git commit -m "Update deepseek-visionary to vX.Y.Z"
git push && gh pr create --repo zed-industries/extensions --fill
```

## 自动化

官方文档提到有社区 GitHub Action 可自动更新 submodule 指针 + version，
待发布稳定后可接入（在 release workflow 中追加）。

## 备注

- `extension.wasm` 不需要提交到 `zed-industries/extensions`，官方 CI 会自行编译验证
- PR 合并后由 Zed 自动打包发布，无需手动操作
- 审核未通过时按反馈修改后重新 push 到同一 PR 即可

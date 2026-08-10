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

## 自动化（tag 触发自动提 PR）

每次打 tag（`vX.Y.Z`）时，自动把新版本同步到 `zed-industries/extensions`
并创建 PR。使用官方文档推荐的社区 Action `huacnlee/zed-extension-action@v2`
（v2 支持扩展在 submodule 子目录、`path` 字段的仓库）。

已配置于 `.github/workflows/zed-extension-release.yml`：

```yaml
on:
  push:
    tags:
      - "v*"

jobs:
  update-zed-extension:
    runs-on: ubuntu-latest
    steps:
      - uses: huacnlee/zed-extension-action@v2
        with:
          extension-name: deepseek-visionary
          push-to: xlight/extensions
        env:
          COMMITTER_TOKEN: ${{ secrets.COMMITTER_TOKEN }}
```

### 启用前置（一次性）

1. **首次上架 PR 已合并**（自动化只负责后续版本更新）
2. **配置 `COMMITTER_TOKEN` secret**：
   - GitHub → Settings → Developer settings → Personal access tokens → Tokens (classic)
   - 勾选 `repo` 与 `workflow` scopes，生成后填入仓库
     Settings → Secrets and variables → Actions → `COMMITTER_TOKEN`

### 发版约定（重要）

打 tag **前**必须同步 `crates/visionary-zed-ext/extension.toml` 的 `version`
与 workspace `Cargo.toml` 一致——PR 的 `package` check 会校验
`extensions.toml` 登记的 version 与 submodule 内 `extension.toml` 一致，
不一致会被 CI 拒绝。

**推荐用一条命令完成全部版本同步**（避免手动改漏）：

```bash
python3 scripts/bump_version.py <new-version>
# 例如 python3 scripts/bump_version.py 0.3.0
# 同步 Cargo.toml + Cargo.lock + extension.toml，并打印后续 commit/tag 步骤
```

CI（`ci.yml` 的 `version-consistency` job）会在 PR/main 上自动校验
workspace 版本与 `extension.toml` 一致，漏同步会被拦截并提示执行该脚本。

### 局限

- 自动创建 PR 后仍需 **Zed 维护者批准**（fork PR 的 workflow 需要 maintainer 批准）
- PR 合并时机由 Zed 团队决定，自动化只能减少重复劳动，不能保证合并
- PR 创建后如需修改，手动更新 fork 分支即可

## 备注

- `extension.wasm` 不需要提交到 `zed-industries/extensions`，官方 CI 会自行编译验证
- PR 合并后由 Zed 自动打包发布，无需手动操作
- 审核未通过时按反馈修改后重新 push 到同一 PR 即可

# CLI 用法

`visionary-cli` 除 `mcp-stdio`（MCP stdio serve）子命令外，提供完整的命令行子命令面，覆盖与 MCP 工具等价的核心能力。适合终端交互、脚本与 AI agent 调用（配合 `skills/visionary-cli/SKILL.md`）。

## 子命令一览

| 子命令 | 对应 MCP 工具 | 说明 |
|--------|---------------|------|
| `vision <image...>` | `deepseek_vision` | 分析一张或多张图片（完整 vision 流水线；多图一次上传联合分析） |
| `ocr <image...>` | `deepseek_ocr` | 提取图片中的文字（等价 `vision --model-type ocr`，纯 OCR 管道） |
| `status` | `deepseek_vision_status` | 轻量鉴权状态检查 |
| `login` | `deepseek_vision_login` | 浏览器自动登录 |
| `logout` | `deepseek_vision_logout` | 清除保存的凭据（池中账号不受影响） |
| `accounts <action>` | `deepseek_vision_accounts` | 账号池管理：`list` / `status` / `add` / `remove` / `use` / `migrate`（`add` / `migrate` 不暴露给 MCP 工具） |
| `doctor` | — | 完整环境诊断（平台/浏览器/config 权限/鉴权；池模式下只报概况，不逐账号探测） |
| `init [agent]` | — | 引导接入 AI agent（opencode / codex / claude / claude-desktop / cursor / dsh） |
| `mcp-stdio` | — | 显式启动 MCP stdio serve 模式 |
| （无参数） | — | 输出 help 用法（退出码 2，不进入 serve） |

## `vision` 参数

```
visionary-cli vision <image>... [--prompt <q>] [--thinking] [--continue-conversation] [--session-id <id>] [--stream|--no-stream] [--model-type <vision|ocr>] [--json]
```

- `image`：一个或多个本地路径 / base64 / data URI；多图一次上传、fork、联合分析（与网页端多图行为一致）。`-`（从 stdin 读取全部字节）仅限单图
- `--prompt`：对图片的问题（默认：请详细描述这张图片中的内容）
- `--thinking`：启用 DeepThink 深度思考
- `--continue-conversation`：续聊，复用上一次会话（可对比多张图片）
- `--session-id`：显式复用指定会话（优先于 `--continue-conversation`）
- `--stream` / `--no-stream`：强制流式 / 强制原子输出（覆盖 TTY 检测默认）
- `--model-type`：上传管道模型类型（默认 `vision`；`ocr` 走纯文字提取管道，等价 `ocr` 子命令）
- `--json`：原子 JSON 输出（禁用流式；与 `--stream` 互斥）

## 输出模式

`vision` 的输出模式由 **stdout 是否 TTY** 与显式开关共同决定：

| 场景 | 默认行为 | 输出形态 | 消费方 |
|------|----------|----------|--------|
| stdout 是 TTY | 文本流式 | completion 增量逐块打印 + 结尾 `[session_id: xxx]` | 终端人 |
| stdout 非 TTY | 原子文本 | 完整文本 + 结尾 session 提示 | 管道/捕获兜底 |
| `--json` | 原子 JSON | `{"text", "session_id", "parent_message_id"}` | 脚本 / agent（推荐） |

- `--stream` 强制流式、`--no-stream` 强制原子，覆盖 TTY 检测默认
- `--json` 恒为原子输出；`--json` 与 `--stream` 互斥（同时指定报错退出非零）
- `--stream` 与 `--no-stream` 互斥

## `--json` 输出形状

成功：

```json
{
  "text": "图片内容描述……",
  "session_id": "abc123",
  "parent_message_id": "msg456"
}
```

失败（未登录、图片读取失败、流水线错误）：

```json
{ "error": "未登录：请先运行 `visionary-cli login` 自动登录……" }
```

## `ocr` 子命令

```text
visionary-cli ocr <image>... [--prompt <q>] [--thinking] [--continue-conversation] [--session-id <id>] [--stream|--no-stream] [--json]
```

`ocr` 是纯文字提取管道（等价 `vision --model-type ocr`，共享同一 handler），参数面对齐 `vision`，但不暴露 `--model-type`（恒为 ocr）。语义差异：

- **默认 prompt**：`请原样输出图片中的文字内容`（`vision` 默认为详细描述；可用 `--prompt` 覆盖）
- **图片无文字**（服务端 `CONTENT_EMPTY`）：退出非零并输出业务提示「图片中未提取到文字（服务端 OCR 提取为空）」（`--json` 时 `{"error": ...}`）；`vision --model-type ocr` 行为相同
- **输出形状**：`--json` 与 `vision --json` 完全一致（`{"text", "session_id", "parent_message_id"}`）

```bash
# 提取截图/文档图片中的文字
visionary-cli ocr screenshot.png

# 结构化提取
visionary-cli ocr scan.png --json

# 与 vision 等价
visionary-cli vision scan.png --model-type ocr --json
```

## `status` 与 `--json`

```
visionary-cli status
visionary-cli status --json
```

默认文本输出：Authenticated / Token configured / smidV2 / Base URL / Token validation。

`--json` 输出：

```json
{
  "authenticated": true,
  "token_configured": true,
  "smid_v2": true,
  "base_url": "https://chat.deepseek.com",
  "token_valid": true
}
```

token 未配置或探针失败时：`status` 退出码非零（无论是否 `--json`）；`--json` 模式仍输出完整状态 JSON（`token_valid: false`），调用方以退出码判断可用性、以 JSON 判断细节。

## `login` 的静音感知与换号

登录成功后，CLI 会用**只读探针**（`GET /api/v0/users/current`）判定账号健康。
若账号被 DeepSeek 上传风控静音（`chat.is_muted = 1`），行为是：

- **浏览器窗口保持打开**（不直接关窗）；
- **stderr** 输出一段换号指引，包含账号标识与静音截止的绝对 UTC 时间；
- 浏览器页面顶部注入一条红色横幅，提示「该账号已被静音，请换号」；
- 流程继续等待——在同一窗口里登出该账号、换登另一个账号后，登录会自动继续并保存新凭据。

```
$ visionary-cli login
The account you signed in to is MUTED (upload risk control).
- Account: xbl*****ht@gmail.com (id: df0b68f1-...)
- Muted until: 2026-11-15 10:28 UTC (unix 1794738505)
The browser window is still open. Please log out of this account in that window and sign in with another one;
the login continues automatically once a usable account is detected.
```

要点：

- 提示按**账号**去重：同一账号刷新 token 不会重复刷屏，换成新账号后允许再次提示；
- 换号判定以探针返回的账号 `id` 为准，**不是**以 token 值判断（同一账号刷新也会轮换 token）；
- 等待超时仍会关窗并报错，错误里带上静音截止时间；
- 提示文案与 MCP 工具 `deepseek_vision_login` **完全一致**（同一份实现）。

### Chrome 无法被程序拉起时（低完整性级别）

`login` 由 chromiumoxide 直接启动 Chrome（专用 profile + `--remote-debugging-port`）。
若启动立刻以 `0x80000003`（STATUS_BREAKPOINT，stderr 为空）退出，几乎总是**完整性级别**问题：

DSH / Codex 沙箱会给**工作区目录**打上 `Mandatory Label\Low`（带 `(OI)(CI)` 继承）。
工作区里**新构建出来的** `visionary-cli.exe` 因此继承同一个 Low 标签 → 进程以 Low 完整性运行 →
它派生出的 `chrome.exe` 同样只能以 Low 完整性启动 → Chrome 初始化渲染进程沙箱时 CHECK 失败 →
`__debugbreak()` 直接退出。

判别方法（同一个二进制文件，只改标签，其它条件完全相同）：

```powershell
# Low 标签 → Chrome 以 0x80000003 崩溃
icacls .\target\release\visionary-cli.exe

# 改成 Medium → Chrome 立刻能正常拉起
icacls .\target\release\visionary-cli.exe /setintegritylevel M
```

仓库里提供了现成脚本（不需要管理员权限，只需对源文件有读权限、对输出目录有写权限）：

```powershell
# 默认：把 target\release\visionary-cli.exe 复制成 target\release\visionary-cli.medium.exe 并改标签
powershell -ExecutionPolicy Bypass -File .\scripts\fix-integrity.ps1

# 指定源与输出
powershell -ExecutionPolicy Bypass -File .\scripts\fix-integrity.ps1 -Path <源 exe> -Out <目标 exe>
```

> **脚本是「复制一份再改标签」，不是原地修改。** cargo 产出的顶层 exe 与
> `deps\visionary_server.exe` 是**硬链接**（同一 MFT 条目），标签共享；
> 原地改成 Medium 会把 `deps` 副本也变成 Medium。而构建工具本身跑在 Low 完整性下，
> 链接器就再也打不开那个文件了（`LNK1104: 无法打开文件 ...\deps\visionary_server.exe`），
> 下次构建直接失败。复制出来的 `.medium.exe` 是独立文件，不影响构建目录。
>
> 每次重新构建后产物会重新继承目录的 Low 标签，需要再跑一次该脚本。
> npm 全局安装的版本不在沙箱工作区内，不受此影响，因此**已发布的 v0.7.8 一直正常**。

程序侧会自我诊断：`login` 启动浏览器失败且当前进程低于 Medium 完整性时，直接报出这个原因和修复命令；
`visionary-cli doctor` 也会把完整性级别列为一项检查。

### 接管已有 Chrome 调试实例（备用路径）

当浏览器启动失败但**不是**完整性级别问题时，`login` 仍会尝试**接管**本机已有的 Chrome 调试实例
（探测本次分配的端口，兜底 `9222`），继续完成登录。接管模式下**不会**关闭你手动启动的浏览器。

若端口上也没有调试实例，错误会打印可直接复制的启动命令（含实际端口与 profile 路径）。按 shell 语法选一种执行：

```powershell
# PowerShell（注意开头的 & ，否则会被当成字符串解析）
& "C:\Program Files\Google\Chrome\Application\chrome.exe" `
  --remote-debugging-port=9222 `
  --user-data-dir="$HOME/.deepseek-visionary/browser"
```

```bash
# cmd / bash
"C:\Program Files\Google\Chrome\Application\chrome.exe" \
  --remote-debugging-port=9222 \
  --user-data-dir="$HOME/.deepseek-visionary/browser"
```

保持该窗口打开，重跑 `visionary-cli login` 即可完成登录。程序日志会打印实际端口与 profile 路径，请以输出为准。

## 账号池与轮询

多账号能力由 `~/.deepseek-visionary/accounts.json`（账号池）承载。池不存在时行为与 v0.7.8 **完全一致**（单账号模式）。

```json
{
  "accounts": [
    {
      "id": "df0b68f1-f8f2-4f2c-860d-98873121e155",
      "label": "xbl*****ht@gmail.com",
      "user_token": "…",
      "smid_v2": "…",
      "cf_clearance": "…",
      "status": "healthy",
      "mute_until": null,
      "last_used": "2026-11-08T18:00:00Z"
    }
  ],
  "cursor": 1,
  "strategy": "rotate",
  "pinned": null,
  "removed": []
}
```

- `id`：账号稳定标识（只读探针 `GET /api/v0/users/current` 返回的 uuid，跨 token 轮换不变）
- `status`：`healthy` / `muted` / `expired`
- `mute_until`：静音截止时间（unix 秒）；到期后自动视为可用（惰性恢复）
- `cursor`：轮询游标（**落盘**——CLI 是一次性进程，落盘才能跨进程继续轮换）
- `pinned`：固定账号 id（至多一个）；固定生效时跳过轮询直选该账号
- `removed`：删除墓碑——防止其它进程把已删账号写回而"复活"

### 轮询语义

- **轮询 = 换凭据，不开浏览器**：每次分析请求前从池中取下一个可用账号，仅在内存里热替换凭据
- 账号被静音（`biz_code=14`）或凭据失效（`40003`）时：标记不可用 → 自动换下一个账号重试该次请求
- 瞬时错误（网络抖动 / 超时）**不会**标记账号不可用——它反映链路而非账号状态
- 固定账号不可用时**仍然只用它**并返回明确错误（含静音截止时间），不静默回退轮询——固定是用户的排障手段，静默换号会让排障结论失效
- 会话按账号隔离（`session.json` 按账号分桶）：换号后不会命中前一账号的会话；显式传入的 `--session-id` 原样复用
- 上传限速是**全局**的（进程级单例）：换账号不等于换配额，`DEEPSEEK_UPLOAD_MIN_INTERVAL` 仍然生效
- `doctor` 在池模式下只报告池概况与当前使用账号；逐账号联网探测是 `accounts status` 的职责

### 单账号兼容

没有 `accounts.json` 时：凭据只来自 `config.json`（环境变量 > 文件），会话仍是单条记录，行为与旧版一致。

池存在时：

- `DEEPSEEK_USER_TOKEN` / `DEEPSEEK_SMIDV2` / `DEEPSEEK_CF_CLEARANCE` **不参与选号**（也不登记为账号）——`status` 会明确提示"环境变量已忽略"
- `logout` 只清空兼容用的单账号配置，**账号仍在池中**继续轮询；要彻底移除某个账号请用 `accounts remove <id>`
- `status` 的退出码以「池中有无可用账号」为准：兼容配置为空或探针失败但池里有可用账号时不会退出非零

### `accounts` 子命令组

```bash
visionary-cli accounts list          # 纯读列出账号（不发起网络请求）
visionary-cli accounts list --json   # 机器可读（PoolView 形状）
visionary-cli accounts status        # 对每个账号并发只读探针，报告健康状态
visionary-cli accounts add           # 打开浏览器添加一个账号
visionary-cli accounts remove <id>   # 移除账号（写墓碑，防其它进程写回复活）
visionary-cli accounts use <id>      # 固定账号（暂停轮询，始终用这个号）
visionary-cli accounts use --clear   # 取消固定，恢复轮询
visionary-cli accounts migrate       # 存量用户一次性导入（幂等、非破坏）
```

所有子命令都支持 `--json`，输出形状统一为 `PoolView`（CLI / MCP / 插件三端同源）：

```json
{
  "enabled": true,
  "accounts": [
    {
      "id": "df0b68f1-f8f2-4f2c-860d-98873121e155",
      "label": "xbl*****ht@gmail.com",
      "status": "healthy",
      "mute_until": null,
      "last_used": "2026-11-08T18:00:00Z",
      "pinned": false
    }
  ],
  "total": 1,
  "healthy": 1,
  "muted": 0,
  "expired": 0,
  "pinned": null,
  "current": "df0b68f1-f8f2-4f2c-860d-98873121e155",
  "env_ignored": false
}
```

- 视图**不含 token 明文**
- `current` 口径三端统一：固定账号 > 最近使用（`last_used` 最大）> 空
- `accounts list` 是**纯读**；只有 `accounts status` 才对每个账号发只读探针（并发执行，整体耗时 ≈ 单次探针）
- `accounts add` 会打开浏览器；若浏览器 profile 里仍是上一个账号的登录态，会明确报告「该账号已在池中，本次未新增账号」并提示先在浏览器里登出——SHALL NOT 谎报添加成功
- `accounts remove` / `use` 会先给出目标账号标签再落盘；`remove` 同时写墓碑
- 池为空但 `config.json` 里有凭据时，CLI 会提示可以执行 `accounts migrate`
- 池存在但**零个可用账号**时，会给出恢复路径（等静音到期 / 登记新号 / 手动配置凭据），与「池未启用」区分

`status --json` 额外带一个 `pool` 子对象（既有 5 个顶层字段保持不变）：

```json
{
  "authenticated": true,
  "token_configured": true,
  "smid_v2": false,
  "base_url": "https://chat.deepseek.com",
  "token_valid": true,
  "pool": {
    "enabled": true,
    "total": 2,
    "healthy": 1,
    "muted": 1,
    "expired": 0,
    "current": "df0b68f1-f8f2-4f2c-860d-98873121e155",
    "env_ignored": true
  }
}
```

池模式下顶层 `token_configured` / `smid_v2` / `token_valid` 描述的是**当前使用账号**
（固定 > 最近使用 > 无），不是被忽略的环境变量；`pool.env_ignored` 为 `true` 表示
检测到 `DEEPSEEK_USER_TOKEN` 等环境变量但它们**未参与选号**。
### 存量用户的一次性迁移

```bash
visionary-cli accounts migrate
```

- **幂等**：池中已有同一账号标识则跳过，不产生重复条目
- **非破坏**：不会删除或清空 `config.json` 里的原有凭据
- **显式触发**：不会在启动路径自动执行
- 需要联网（用只读探针换账号稳定标识）；探针失败会明确报错并**不**写入占位标识

> 回退：删除 `accounts.json` 即回到单账号模式。回退后单账号模式使用的是 `config.json` 当时的凭据（由**最后一次登记**决定，未必是迁移前的账号）；若它已静音或失效，请重新执行 `accounts migrate`。
## 退出码约定

| 场景 | 退出码 |
|------|--------|
| 成功 | 0 |
| 失败（未登录 / 图片读取失败 / 流水线错误 / 登录失败） | 1 |
| 参数冲突（如 `--stream --no-stream`、`--json --stream`）或用法错误 | 2（clap） |

## 示例

```bash
# 终端交互：流式输出（打字机效果）
visionary-cli vision screenshot.png

# 指定问题 + DeepThink
visionary-cli vision img.png --prompt "图中有什么？" --thinking

# 脚本/agent：结构化输出
visionary-cli vision img.png --json

# 管道输入
cat img.png | visionary-cli vision - --json

# 会话续聊
visionary-cli vision img2.png --continue
visionary-cli vision img3.png --session-id <上一次的 session_id>

# 登录前预检
visionary-cli status --json || visionary-cli login

# 强制原子输出（即使 stdout 是 TTY）
visionary-cli vision img.png --no-stream
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_ACCOUNTS_TIMEOUT` | 账号池批量健康刷新的整体预算秒数（默认 30；单次探针 ≤15s，各探针并发执行） |

与 MCP 模式共享：`DEEPSEEK_USER_TOKEN` / `DEEPSEEK_SMIDV2` / `DEEPSEEK_CF_CLEARANCE` / `DEEPSEEK_BASE_URL` / `DEEPSEEK_LOGIN_TIMEOUT`。

账号健康探针：`status` / `doctor` / MCP 状态工具与登录流程共用**同一只读探针**
（`GET /api/v0/users/current`）——纯 GET，**不发起上传、不求解 PoW、不消耗配额**。
探针的超时由调用方按场景给定（`status` / `doctor` 为 15 秒；登录流程按模式取总预算的 1/3，
上限 15 秒），因此没有单独的探针开关环境变量。


登录页语言：默认跟随系统 locale（`LC_ALL` / `LC_MESSAGES` / `LANG`，如 `zh_CN.UTF-8` → 中文版），可用 `DEEPSEEK_LOGIN_LANG` 手动指定（如 `zh-CN` / `en` / `ja`）。

token 自动续期：DeepSeek 网页端每次加载页面都会轮换 `userToken` 并作废旧 token，而 CLI 存的只是快照。检测到鉴权失败（`biz_code=40003` / HTTP 401）时，CLI 会用持久化的浏览器 profile **自动续期并重试一次**（会短暂拉起一次浏览器窗口，通常几秒内完成）。

- `DEEPSEEK_AUTO_REFRESH`：设为 `0` 关闭自动续期（默认开启）
- `DEEPSEEK_REFRESH_TIMEOUT`：自动续期的登录等待超时秒数（默认 90）

上传限速：同一进程内的连续上传（多图联合分析）默认间隔 5 秒，避免触发 DeepSeek 的上传风控（`biz_code=14 user is muted`，账号会被静音数天，届时错误信息里会给出解除时间的 UTC 绝对时间）。

- `DEEPSEEK_UPLOAD_MIN_INTERVAL`：两次上传之间的最小间隔秒数（默认 5，`0` 关闭限速）

## 与 MCP 工具的关系

- `vision` / `ocr` / `status` / `login` / `logout` / `accounts` 六个子命令组与 MCP 工具共享同一核心实现（`pipeline.rs` / `login.rs` / `auth.rs` / `accounts.rs`），无两套漂移代码
- MCP 侧对应 6 个工具（新增 `deepseek_vision_accounts`：`list` / `refresh` / `remove` / `pin`）；`add` / `migrate` 仅 CLI 暴露。账号标识走位置参数，三端（CLI / MCP / DSH 插件）共用同一 `PoolView` 形状
- MCP 工具行为完全不变；CLI 是同一能力的另一访问入口
- 日志统一走 stderr（serve 模式 stdout 是 MCP 协议通道，CLI 子命令 stdout 只输出结果）

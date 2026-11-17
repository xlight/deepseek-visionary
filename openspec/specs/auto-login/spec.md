# auto-login Specification

## Purpose
定义浏览器自动登录与凭据管理能力：通过 CDP 控制 Chrome 系浏览器完成 DeepSeek 登录，自动抓取并安全持久化凭据，支持热重载与手动配置兜底，零手动复制 token。

## Requirements
### Requirement: 浏览器自动登录

MCP 服务 SHALL 提供 `deepseek_vision_login` 工具：启动 Chrome 系浏览器（专用 profile + 远程调试端口）打开 chat.deepseek.com，等待用户登录完成后，通过 CDP 读取 `localStorage.userToken` 与 `smidV2`、`cf_clearance` cookie，并持久化到 `~/.deepseek-visionary/config.json`（键名 `user_token` / `smid_v2` / `cf_clearance`）。

抓取到凭据后，服务 SHALL 先探测账号健康（见 `account-health` 的「账号健康只读探针」）：账号健康则关闭浏览器并返回成功；账号被静音则 SHALL 保持浏览器打开、提示用户换号，并继续等待用户在同一窗口换号登录——换号后重新探测，健康则正常保存并关闭浏览器。

#### Scenario: 首次登录
- **WHEN** 用户调用登录工具且本地无登录会话
- **THEN** 弹出浏览器窗口并立即返回（后台持续监听登录），用户完成登录后自动抓取并保存凭据，可通过状态工具查询进度

#### Scenario: 登录进行中查询状态
- **WHEN** 登录尚未完成时用户调用状态工具
- **THEN** 状态工具报告"等待登录完成"而非错误

#### Scenario: profile 已有登录会话
- **WHEN** 专用浏览器 profile 已存在有效登录
- **THEN** 直接读取 token，无需再次登录

#### Scenario: 程序无法直接拉起 Chrome 时接管已有调试实例
- **WHEN** 直接启动 Chrome 失败（如 Chrome 以断点异常退出），但本机调试端口上已有用户手动启动的带 `--remote-debugging-port` 的 Chrome
- **THEN** 登录流程 SHALL 连接该实例继续完成登录，且 SHALL NOT 关闭用户手动启动的浏览器窗口

#### Scenario: 无可用浏览器
- **WHEN** 系统未找到 Chrome 系浏览器
- **THEN** 登录工具返回指引，回退到手动配置路径

#### Scenario: 抓取 cf_clearance cookie
- **WHEN** 浏览器会话中存在 cf_clearance cookie
- **THEN** 该 cookie 随凭据一并持久化到配置文件

#### Scenario: 登录到被静音账号
- **WHEN** 用户完成登录但探测显示该账号被静音
- **THEN** 登录工具保持浏览器打开、向 CLI stderr / MCP 通道输出换号指引，并**在浏览器页面内注入醒目的提示横幅**（说明当前账号、静音截止时间、需登出并换用其他账号），等待用户在同一窗口换号；换号后自动完成凭据保存

#### Scenario: 静音提示出现在用户可见的浏览器页面
- **WHEN** 检测到账号静音，且浏览器为「接管用户手动启动的 Chrome」模式
- **THEN** 提示横幅 SHALL 注入到该浏览器**所有**页面（不只程序新建的标签页），确保用户正在操作的页面也能看到换号提示；页面为 SPA 时 SHALL 每轮重新注入以对抗导航/重载清空

#### Scenario: 自动续期遇静音不等待换号
- **WHEN** 自动续期流程探测到账号被静音
- **THEN** 立即以静音错误失败并关闭浏览器，SHALL NOT 保持窗口等待换号

### Requirement: 凭据安全存储
服务 SHALL 将凭据写入 `~/.deepseek-visionary/config.json` 并设置限制性权限（0600），日志中 SHALL NOT 输出 token 明文。

#### Scenario: 配置文件权限
- **WHEN** 登录成功写入凭据
- **THEN** 配置文件权限为 0600

#### Scenario: 日志脱敏
- **WHEN** 服务打印调试日志
- **THEN** 日志中不包含 token 明文

### Requirement: 凭据热重载
服务 SHALL 在登录工具成功后热重载凭据，无需重启进程即可用于后续 vision 调用。

#### Scenario: 登录后立即使用
- **WHEN** 登录工具返回成功
- **THEN** 后续 `deepseek_vision` 调用直接使用新凭据

### Requirement: 退出登录
MCP 服务 SHALL 提供 `deepseek_vision_logout` 工具，清除本地存储的凭据。

#### Scenario: 清除凭据
- **WHEN** 用户调用退出登录工具
- **THEN** 本地配置文件中的 token 被清除，状态检查显示未认证

### Requirement: 手动配置兜底
服务 SHALL 支持通过 `DEEPSEEK_USER_TOKEN`（及可选 `DEEPSEEK_SMIDV2`、`DEEPSEEK_CF_CLEARANCE`）环境变量或手动编辑配置文件提供凭据，此时不依赖浏览器登录。

#### Scenario: 环境变量方式
- **WHEN** 用户已设置 `DEEPSEEK_USER_TOKEN` 环境变量
- **THEN** 服务直接使用该凭据完成鉴权


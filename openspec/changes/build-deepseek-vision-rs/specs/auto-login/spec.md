## ADDED Requirements

### Requirement: 浏览器自动登录
MCP 服务 SHALL 提供 `deepseek_vision_login` 工具：启动 Chrome 系浏览器（专用 profile + 远程调试端口）打开 chat.deepseek.com，等待用户登录完成后，通过 CDP 读取 `localStorage.userToken` 与 `smidV2`、`cf_clearance` cookie，并持久化到 `~/.deepseek-vision/config.json`（键名 `user_token` / `smid_v2` / `cf_clearance`）。

#### Scenario: 首次登录
- **WHEN** 用户调用登录工具且本地无登录会话
- **THEN** 弹出浏览器窗口并立即返回（后台持续监听登录），用户完成登录后自动抓取并保存凭据，可通过状态工具查询进度

#### Scenario: 登录进行中查询状态
- **WHEN** 登录尚未完成时用户调用状态工具
- **THEN** 状态工具报告"等待登录完成"而非错误

#### Scenario: profile 已有登录会话
- **WHEN** 专用浏览器 profile 已存在有效登录
- **THEN** 直接读取 token，无需再次登录

#### Scenario: 无可用浏览器
- **WHEN** 系统未找到 Chrome 系浏览器
- **THEN** 登录工具返回指引，回退到手动配置路径

#### Scenario: 抓取 cf_clearance cookie
- **WHEN** 浏览器会话中存在 cf_clearance cookie
- **THEN** 该 cookie 随凭据一并持久化到配置文件

### Requirement: 凭据安全存储
服务 SHALL 将凭据写入 `~/.deepseek-vision/config.json` 并设置限制性权限（0600），日志中 SHALL NOT 输出 token 明文。

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

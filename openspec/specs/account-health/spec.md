# account-health Specification

## Purpose
账号健康只读探针：以只读 GET /api/v0/users/current 判定账号健康与静音状态，作为登录、状态检查与账号池的统一判定入口。

## Requirements

### Requirement: 账号健康只读探针

服务 SHALL 提供账号健康探针，经 `GET /api/v0/users/current`（`Authorization: Bearer <token>`）查询当前凭据对应的账号，复用既有 `ApiClient` 与 `ApiEnvelope` 解包路径，账号档案位于 `data.biz_data`。

探针 SHALL 为纯只读调用：SHALL NOT 发起上传、SHALL NOT 求解 PoW、SHALL NOT 消耗配额。

探针 SHALL 返回：静音标记（`chat.is_muted`）、静音截止时间（`chat.mute_until`，unix 秒；缺失时表示为「未知截止时间」）、账号稳定标识（`id`，uuid，跨 token 轮换不变）、展示标签（`email` → `mobile_number` → `id_profile.name` 取首个非空值）。

token 失效时 SHALL 表现为鉴权失败（HTTP 200 且顶层 `code=40003`），SHALL NOT 表现为静音。

#### Scenario: 探测到账号被静音
- **WHEN** 对一个已被上传风控静音的账号调用探针
- **THEN** 返回「被静音」判定与静音截止时间（unix 秒），账号稳定标识与展示标签一并返回

#### Scenario: 探测到账号健康
- **WHEN** 对一个未被静音的账号调用探针
- **THEN** 返回「健康」判定，账号稳定标识与展示标签一并返回，SHALL NOT 产生任何上传或配额消耗

#### Scenario: 探针遇到失效 token
- **WHEN** 用已失效的 token 调用探针
- **THEN** 表现为鉴权失败（可由既有 `AuthFailure` 路径捕获），SHALL NOT 被误判为账号静音

#### Scenario: 探针字段缺失时降级
- **WHEN** 探针响应中缺少 `chat` 字段（接口契约变更）
- **THEN** 按「未静音」处理并记录告警，SHALL NOT 阻断登录流程

#### Scenario: 探针单次超时
- **WHEN** 探针请求超时
- **THEN** 返回可区分的超时错误，SHALL NOT 阻断登录等待循环（等待方按「未知健康」降级并继续等待）

### Requirement: 静音错误类型

服务 SHALL 以独立的错误类型表示「账号被静音」，与既有的鉴权失败（`AuthFailure`）与瞬时错误（`TransientFailure`）并列。

静音错误 SHALL 携带可读的静音截止时间（unix 秒，缺失时表示为未知）。错误信息 SHALL 包含绝对 UTC 时刻形式的静音截止时间（复用既有 `format_epoch_utc`），使调用方无需自行换算。

静音错误 SHALL NOT 被当作鉴权失败——对静音账号刷新 token 无效。

#### Scenario: 上传被静音时的错误可区分
- **WHEN** 上传接口返回 `biz_code=14`（`user is muted`）
- **THEN** 错误以静音类型呈现，并携带静音截止时间的绝对 UTC 时刻

#### Scenario: 静音错误不被误判为鉴权失败
- **WHEN** 判断一个静音错误是否为鉴权失败
- **THEN** 判定为否，SHALL NOT 触发 token 续期路径

### Requirement: 登录时静音感知

浏览器登录流程在抓取到凭据后 SHALL 先探测账号健康，再决定后续行为：

- 账号健康 → 保存凭据、关闭浏览器、返回成功
- 账号被静音 → **SHALL NOT 关闭浏览器**，SHALL 提示用户换号，并继续等待用户操作

提示 SHALL 同时出现在两处：

- **CLI / MCP 输出**：提示当前账号被静音及其截止时间，引导用户在已打开的浏览器窗口中登出并换用其他账号
- **浏览器页面**：在页面内注入可见横幅，告知当前账号被静音及截止时间

提示后流程 SHALL 继续等待：用户在同一浏览器窗口换号登录后，SHALL 重新抓取凭据并重新探测；探测健康则正常保存并关闭浏览器。等待超时仍未完成换号时 SHALL 关闭浏览器并报错，错误信息 SHALL 包含静音截止时间。

#### Scenario: 登录到被静音账号时保持浏览器打开
- **WHEN** 用户在浏览器中登录的账号被静音
- **THEN** 浏览器窗口保持打开，输出与页面横幅均提示该账号被静音及截止时间、引导换号，SHALL NOT 立即关闭浏览器

#### Scenario: 换号后自动完成登录
- **WHEN** 用户在保持打开的浏览器窗口中登出并换用另一个账号登录
- **THEN** 流程重新抓取该账号的凭据并重新探测，健康则保存该凭据、关闭浏览器、登录成功

#### Scenario: 等待换号超时
- **WHEN** 提示换号后超过登录超时仍未换用可用账号
- **THEN** 关闭浏览器并返回错误，错误信息包含静音截止时间

### Requirement: 换号身份判定

登录流程 SHALL 以探针返回的账号稳定标识判定账号身份，SHALL NOT 因 token 值变化而误判——DeepSeek 会在每次网页加载时轮换 `localStorage.userToken`，token 变化不等于账号变化。

换号确认 SHALL 经「token 变化 → 重新探测 → 比对账号稳定标识」两段判定，SHALL NOT 单凭 token 变化即认定换号。

#### Scenario: 换号后不被误判
- **WHEN** 用户换号登录，流程重新抓取凭据
- **THEN** 以账号稳定标识判定账号身份，SHALL NOT 因 token 值变化而把新账号误当作原账号（或反之）

#### Scenario: 同账号刷新不被误判为换号
- **WHEN** 用户在浏览器中刷新页面导致 token 轮换，但登录的仍是同一账号
- **THEN** 重新探测后比对账号稳定标识判定为同一账号，不重复输出静音提示、不等待换号

### Requirement: 登录流程的关窗与模式区分

浏览器 SHALL 仅在登录流程的终态关闭：登录成功、超时、以及自动续期模式下的静音。被静音且处于交互模式时，浏览器 SHALL 保持打开等待用户换号——实现上由等待循环在静音分支**不返回**达成，关窗调用点 SHALL 保持在流程外层的原位（以保留异常路径的清理兜底），SHALL NOT 把关窗逻辑搬进状态机分支。

自动续期与交互登录 SHALL 由显式模式区分：交互登录遇静音 SHALL 等待用户换号，自动续期遇静音 SHALL 快速失败并关闭浏览器，SHALL NOT 等待换号。模式区分 SHALL NOT 依赖超时时长数值（超时值可由环境变量配置，不可作为可靠判据）。

页面横幅 SHALL 在每次探测到静音后重新注入，SHALL NOT 依赖单次注入长期存活——换号引发的页面导航或重载会清空注入的节点。横幅注入失败 SHALL NOT 中断流程。

#### Scenario: 自动续期遇静音快速失败
- **WHEN** 自动续期流程探测到账号被静音
- **THEN** 立即以静音错误失败并关闭浏览器，SHALL NOT 等待用户换号

#### Scenario: 页面重载后横幅重新注入
- **WHEN** 用户换号导致页面导航或重载，注入的横幅节点被清空
- **THEN** 下次探测到静音时横幅被重新注入，提示仍然可见

#### Scenario: 浏览器仅在终态关闭
- **WHEN** 登录流程中账号被静音，服务保持浏览器打开等待换号
- **THEN** 浏览器在等待期间 SHALL NOT 关闭；仅在登录成功、超时或自动续期模式快速失败时关闭


### Requirement: 探针作为统一健康判定入口

服务 SHALL 以账号健康只读探针作为 token 有效性与账号健康状态的**唯一**判定入口。既有基于 `POST /api/v0/chat/create_pow_challenge` 的校验路径 SHALL 被取代；`status` 子命令、`doctor` 子命令与 MCP 状态工具 SHALL 全部改用只读探针。

替换后，SHALL NOT 再存在任何为「检查登录状态」而发起的 PoW 挑战请求。CLI `status` 与账号池的批量状态刷新 SHALL 遵循同一判定语义与同一代价模型（均为只读、无配额消耗）。

#### Scenario: 状态检查走只读探针
- **WHEN** 用户执行 `status` 或 `doctor`，或调用 MCP 状态工具
- **THEN** 仅发起只读探针请求，SHALL NOT 发起 PoW 挑战请求

#### Scenario: 判定语义一致
- **WHEN** 同一凭据分别经 CLI `status` 与账号池批量刷新被检查
- **THEN** 两者给出相同的有效性判定与相同的账号健康状态
### Requirement: 探测使用候选凭据

登录流程中的账号健康探测 SHALL 使用**本次抓取到的候选凭据**发起，SHALL NOT 使用配置中当前生效的凭据——候选凭据在探测时点尚未写入配置，若沿用当前凭据，探测判定将属于上一个账号（首次登录则因凭据为空而误报为鉴权失败）。

构造用于探测的配置副本时，SHALL 使用独立的凭据容器，SHALL NOT 复用或克隆现有配置的凭据容器（克隆会共享同一容器，从而改写正在生效的凭据）。

#### Scenario: 首次登录探测的是新账号
- **WHEN** 本地无任何凭据时用户完成登录
- **THEN** 探测使用本次抓到的凭据，返回的是新登录账号的健康状态

#### Scenario: 探测不影响当前生效凭据
- **WHEN** 已有账号在配置中生效时用户重新登录另一个账号
- **THEN** 探测过程 SHALL NOT 改写配置中当前生效的凭据

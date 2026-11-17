# account-management-surface Specification

## Purpose
账号管理三端一致呈现：CLI accounts 子命令组、MCP 账号管理工具与 DSH 插件账号池区块共用同一 PoolView 形状。

## Requirements

### Requirement: 账号管理三端一致呈现

账号池 SHALL 在 CLI、MCP 工具、DSH 插件三端以一致语义呈现。每个账号条目 SHALL 展示：展示标签、状态（健康 / 已静音 / 凭据失效）、静音截止时间（已静音时）、最近使用时刻。

状态语义 SHALL 三端一致：同一账号在任一端显示的状态 SHALL 与其它端一致。

「当前使用的账号」三端 SHALL 取同一口径：有固定账号时为被固定的账号；否则为最近使用的账号；一个都没用过时为空。SHALL NOT 各端自行取不同的候选（如轮询游标指向的下一个）。

三端 SHALL NOT 展示账号的 token 明文或任何可用于直接鉴权的凭据片段。

账号池不存在或为空时，三端 SHALL 呈现「未启用账号池」的明确状态，SHALL NOT 报错。

#### Scenario: 三端状态语义一致
- **WHEN** 某账号在池中被标记为已静音
- **THEN** CLI、MCP 工具返回、DSH 插件面板三处呈现同一状态与同一静音截止时间

#### Scenario: 不展示凭据明文
- **WHEN** 任一端呈现账号列表
- **THEN** 输出中不包含任何账号的 token 明文

#### Scenario: 未启用账号池
- **WHEN** 账号池不存在或为空
- **THEN** 三端呈现「未启用账号池」状态，不报错

### Requirement: CLI accounts 子命令组

`visionary-server` SHALL 提供 `accounts` 子命令组，包含：

- `accounts list`：列出池中账号及其状态
- `accounts status`：批量刷新健康状态后列出（只读探针，SHALL NOT 消耗上传配额、SHALL NOT 启动浏览器）
- `accounts add`：启动浏览器登录新账号并登记进池
- `accounts remove <id>`：移除指定账号
- `accounts use <id>`：固定使用指定账号（后续请求不再轮询），用于排查问题；SHALL 支持取消固定
- `accounts migrate`：把既有单账号配置 `config.json` 中的凭据一次性导入账号池（幂等、非破坏、显式触发）

命令 SHALL 遵循既有约定：成功退出码 0，失败退出码非零，结构化输出可用 `--json`。

#### Scenario: 列出账号
- **WHEN** 用户执行 `visionary-server accounts list`
- **THEN** 列出池中每个账号的标签、状态、静音截止时间与最近使用时刻

#### Scenario: 刷新账号状态
- **WHEN** 用户执行 `visionary-server accounts status`
- **THEN** 各账号状态被批量刷新后展示，全程无上传、无浏览器启动

#### Scenario: 添加账号
- **WHEN** 用户执行 `visionary-server accounts add` 并在浏览器中完成登录
- **THEN** 新账号被登记进池并出现在列表中

#### Scenario: 移除账号
- **WHEN** 用户执行 `visionary-server accounts remove <id>`
- **THEN** 该账号从池中移除，其余账号不受影响

#### Scenario: 固定使用某账号
- **WHEN** 用户执行 `visionary-server accounts use <id>`
- **THEN** 后续分析请求固定使用该账号，不再轮询；用户可取消固定

#### Scenario: 迁移存量凭据
- **WHEN** 池为空而 `config.json` 含有效凭据，用户执行 `visionary-server accounts migrate`
- **THEN** 该凭据被导入池并可参与轮询，输出报告导入结果；`config.json` 原有凭据保持不变

#### Scenario: 重复执行迁移不产生重复条目
- **WHEN** 池中已存在该账号标识，用户再次执行 `visionary-server accounts migrate`
- **THEN** 不新增重复条目，输出报告该账号被跳过及原因


### Requirement: status 子命令报告账号池概况

既有 `status` 子命令 SHALL 扩展，报告账号池概况（账号总数、各状态计数、当前使用的账号），而非仅报告单账号的 token 布尔值；既有字段 SHALL 保持不变。

#### Scenario: status 报告账号池概况
- **WHEN** 用户执行 `visionary-server status`
- **THEN** 输出包含账号池概况（总数、各状态计数、当前账号）与既有的 token 有效性信息
### Requirement: 账号池状态的端间区分

三端 SHALL 区分「账号池未启用」（池不存在或为空）与「账号池已启用但零个可用账号」两种状态，后者 SHALL 给出各账号状态与恢复路径（等待静音到期、登记新账号、手动配置凭据），SHALL NOT 与前者混为一谈。

池存在且检测到环境变量凭据已设置时，命令输出 SHALL 提示「已忽略环境变量，当前按账号池轮询」。

#### Scenario: 池存在但零可用账号
- **WHEN** 账号池存在但其中没有可用账号（全部静音或凭据失效）
- **THEN** 输出明确说明「账号池已启用但无可用账号」，列出各账号状态与恢复路径，与「账号池未启用」的呈现相区分

#### Scenario: 池存在时提示已忽略环境变量
- **WHEN** 账号池存在且环境变量 `DEEPSEEK_USER_TOKEN` 已设置
- **THEN** 输出提示「已忽略环境变量，当前按账号池轮询」

### Requirement: 添加账号绕过登录幂等检查

`accounts add` SHALL 走绕过既有登录幂等检查的入口：既有 `run_login` 在已配置 token 时直接返回「Token already configured」而 SHALL NOT 打开浏览器，若复用则「已有账号时添加第二个号」永远无法进行。 账号池启用时，**MCP 的登录工具 SHALL 同样采用「添加账号」语义**并走该绕过入口——否则池已建立后用户将无法通过 MCP 添加第二个账号。

#### Scenario: 已有账号时仍可添加新账号
- **WHEN** `config.json` 已有凭据，用户执行 `visionary-server accounts add`
- **THEN** 浏览器正常打开并进入登录流程，SHALL NOT 因既有登录的幂等检查而直接返回「已配置」
### Requirement: MCP 账号管理工具

MCP 服务 SHALL 提供 `deepseek_vision_accounts` 工具，支持以下动作：列出账号、刷新健康状态、移除账号、固定使用某账号。动作参数 SHALL 以结构化 schema 表达。

既有 `deepseek_vision_status` 工具 SHALL 扩展账号池信息（总数、各状态计数、当前账号），SHALL 保持既有字段不破坏。

工具 SHALL 返回人类可读文本（与既有工具一致），且 SHALL NOT 包含 token 明文。

工具 SHALL 按固定映射落地到 CLI 子命令（列出 → `accounts list`；刷新 → `accounts status`；移除 → `accounts remove <id>`；固定 → `accounts use <id>`，取消固定 → `accounts use --clear`）。账号标识 SHALL 以位置参数传递，SHALL NOT 使用 `--account-id=` 形式。工具 SHALL NOT 暴露迁移动作。

#### Scenario: 通过工具列出账号
- **WHEN** 调用 `deepseek_vision_accounts` 且动作参数为列出
- **THEN** 返回账号列表，含标签、状态、静音截止时间

#### Scenario: 通过工具刷新状态
- **WHEN** 调用 `deepseek_vision_accounts` 且动作参数为刷新
- **THEN** 各账号状态被刷新后返回，无上传、无浏览器启动

#### Scenario: status 工具含账号池信息
- **WHEN** 调用 `deepseek_vision_status`
- **THEN** 返回值包含账号池概况与既有的 token 有效性字段

### Requirement: 插件 Remote 通道约束

Remote 方法 SHALL 满足宿主的签名约束（单标识符参数、可用 `null`、SHALL NOT 用解构 / 默认值 / rest），且新增方法 SHALL 同步修改 dashboard Service 的声明块、static 赋值块与方法表三处——漏改任一处该方法 SHALL 静默不暴露。**参数表 SHALL 写成纯标识符列表**：`pinAccount` 的参数名是 `id`，取消固定时传入**值** `null`；SHALL NOT 写成 `pinAccount(id | null)`——那不是合法 JS，会被宿主判为签名非法。

调用 Remote 前 SHALL 判空 `ctx.remote.visionaryDashboard`（dashboard 是独立于 settings 门控的第三个 entry）；Remote 返回值 SHALL 兼容 `{ok:true,value}` 与裸值两种形状。

#### Scenario: dashboard row 未启用时不抛错
- **WHEN** dashboard row 未启用（但某个 config entry 已启用），`ctx.remote.visionaryDashboard` 为 undefined
- **THEN** 插件 SHALL 跳过对 Remote 的调用，页面 SHALL NOT 抛错，其余区块与表单行为正常

#### Scenario: 取消固定以 null 值传参
- **WHEN** 用户取消固定某个账号，插件调用固定方法并传入 `null`
- **THEN** 调用成功，池中的固定标记被清除；SHALL NOT 传 `undefined`

#### Scenario: Remote 返回形状兼容
- **WHEN** Remote 调用返回 `{ok:true,value}` 包裹的结果，或返回裸值
- **THEN** 两种形状都能被正确解包，账号池数据正常渲染
### Requirement: DSH 插件账号池区块

DSH 插件 SHALL 在既有 bundle 详情页的仪表盘段内渲染**账号池区块**，展示账号列表（标签 + 状态徽标 + 静音截止时间）与操作入口。

状态徽标 SHALL 视觉区分三种状态（健康 / 已静音 / 凭据失效）。区块 SHALL 提供动作按钮：刷新状态、添加账号、移除账号、固定使用某账号。

账号池数据与动作 SHALL 经既有 dashboard Service（cordis Remote `visionaryDashboard`）暴露给 client，与既有仪表盘区块复用同一传输通道。

状态徽标 SHALL 沿用仪表盘汇总区既有的**手写**样式族（dashboard 汇总并非使用宿主 primitives，宿主亦无 warning / neutral 徽标 token）。

该区块 SHALL NOT 改变既有仪表盘汇总、Vision Tools 表单、Image Bridge 表单的布局与行为。

#### Scenario: 插件面板展示账号池
- **WHEN** 用户打开 `@xlight-oss/visionary-dsh` bundle 详情页
- **THEN** 仪表盘段内出现账号池区块，列出各账号标签与状态徽标

#### Scenario: 从插件刷新账号状态
- **WHEN** 用户点击账号池区块的「刷新状态」
- **THEN** 各账号状态经 dashboard Service 刷新并重新渲染

#### Scenario: 静音账号有独立徽标
- **WHEN** 某账号处于已静音状态
- **THEN** 该条目显示静音徽标与静音截止时间，与健康账号视觉可区分

#### Scenario: 未启用账号池时的插件呈现
- **WHEN** 账号池不存在或为空
- **THEN** 账号池区块呈现「未启用账号池」状态，不报错、不影响其它区块

#### Scenario: 插件端区分零可用账号
- **WHEN** 账号池存在但其中没有可用账号
- **THEN** 账号池区块呈现「无可用账号」并列出各账号状态与恢复路径，与「未启用账号池」相区分

### Requirement: 账号批量刷新的并发与超时

批量刷新各账号健康状态时，服务 SHALL **并发**执行各账号的只读探针，整体耗时 SHALL 约等于单次探针而非账号数倍。单次探针 SHALL 有独立超时（默认 15 秒），整体 SHALL 有独立预算（默认 30 秒）；超预算的探针按探针失败处理，SHALL NOT 影响其它账号。

#### Scenario: 批量刷新不随账号数线性变慢
- **WHEN** 账号池含多个账号且执行批量刷新
- **THEN** 各账号探针并发执行，整体耗时约等于单次探针

#### Scenario: 单个探针超时不影响其它账号
- **WHEN** 批量刷新中某个账号的探针超时
- **THEN** 该账号按探针失败处理，其余账号的状态正常更新

#### Scenario: 插件端添加账号不阻塞面板
- **WHEN** 用户在插件面板触发「添加账号」
- **THEN** host 侧启动登录后立即返回，面板保持可交互并沿用既有登录态轮询呈现等待状态

#### Scenario: 账号变更后分析请求立即使用新池
- **WHEN** 用户经 MCP 工具添加了一个账号，随后发起一次分析请求
- **THEN** 该请求的轮询使用更新后的账号池，新账号可被选中

#### Scenario: 池模式下 MCP 登录可添加第二个账号
- **WHEN** 账号池中已有账号，用户通过 MCP 登录工具发起登录
- **THEN** 浏览器正常打开并可登录另一个账号，不被既有登录的幂等检查挡住

### Requirement: 账号变更后立即生效

MCP 与插件端的账号管理操作经 CLI 子进程落地，改动的是磁盘上的账号池文件。执行方 SHALL 在操作成功后重载账号池，使随后的分析请求立即使用更新后的池，SHALL NOT 继续使用启动时的旧快照。

#### Scenario: 添加账号后立即可被轮询选中
- **WHEN** 用户添加了一个账号，随后发起一次分析请求
- **THEN** 该请求的轮询使用更新后的账号池，新账号可被选中

# dsh-plugin-dashboard Specification

## Purpose
visionary-dsh 插件面板的聚合仪表盘段：在 bundle 详情页两行 entry（vision / bridge）之上展示二进制就绪状态、版本兼容性、登录/会话态、桥接运行态与落盘副本数，并提供诊断动作（重探测二进制、重新登录、清理落盘、自检），数据经 host Service 经 cordis RPC 提供，client 经 `plugins.bundle.config` 槽注册渲染。

## Requirements

### Requirement: 仪表盘段位置与渲染

插件 SHALL 在 bundle 详情页（`plugins.bundle.config` 槽，`key` = `@xlight-oss/visionary-dsh`）注册**唯一一个** bundle 配置段，该段 SHALL 依次渲染三部分内容（其中仪表盘汇总内含账号池区块）：

1. **仪表盘汇总**：二进制就绪状态、版本兼容性、登录/会话态、桥接运行态与落盘副本数，以及 4 个诊断动作按钮（重探测二进制、重新登录、清理落盘、自检）；汇总之后 SHALL 渲染**账号池区块**（见下）
2. **Vision Tools 配置表单**：`modelType` / `binaryPath` / `visionTimeoutMs` + 高级区（`statusTimeoutMs` / `loginTimeoutSeconds` / `accountsTimeoutMs`），带 Save / Discard。高级区各行 SHALL **手写**在表单组件中——插件关闭了设置 schema 的自动表单派生，SHALL NOT 假设新增配置项会自动出现
3. **Image Bridge 配置表单**：`enabled` / `scope` / `mode` + 高级区（`promptTemplate` / `pastedDir` / `retainHours` / `cleanPasted` 触发器），带 Save / Discard

账号池区块 SHALL 位于仪表盘汇总之后、Vision Tools 表单之前，展示账号列表（展示标签 + 状态徽标 + 静音截止时间）与操作入口（刷新状态、添加账号、移除账号、固定 / 取消固定）。账号池数据与动作 SHALL 经既有 dashboard Service（cordis Remote `visionaryDashboard`）暴露给 client，SHALL NOT 新开 `plugins.bundle.config` 段。账号池不存在或为空时，区块 SHALL 呈现「未启用账号池」状态，不报错。

该段 SHALL 仅在 host 起了至少一个 entry 时渲染（`whileServed` 门控）；host 未起任何 entry 时不渲染（无空壳）。dashboard 是**第三个** entry（`whileServed` 只门控两个 settings 命名空间），因此 dashboard row 未启用时 `ctx.remote.visionaryDashboard` 可能为 undefined——调用 Remote 方法前 SHALL 判空，SHALL NOT 因 remote 缺失而抛错。两份表单的 injected faces SHALL 命名空间隔离（`useVisionCard`/`visionEdit`/`visionSave`/`visionDiscard` 与 `useBridgeCard`/`bridgeEdit`/`bridgeSave`/`bridgeDiscard`），以避免在单一 `inject()` 返回值中 key 冲突。`inject()` SHALL 返回对象（含 `remote` / `hooks` / 命名空间前缀的 edit/save 方法），SHALL NOT 返回数组。

#### Scenario: bundle 详情页出现内联仪表盘与两份表单
- **WHEN** 用户打开 Plugins 面板 → `@xlight-oss/visionary-dsh` bundle 详情页
- **THEN** 在 bundle 描述之下、entry 行之上出现单一配置段，依次含：仪表盘汇总（4 区块 + 4 动作 + 账号池区块）、Vision Tools 表单、Image Bridge 表单

#### Scenario: host 未起任何 entry 时不渲染
- **WHEN** bundle 已安装但两个 entry 均未运行（如被 profile 禁用）
- **THEN** bundle 详情页不出现该配置段（无空壳、无 loading 占位）

#### Scenario: 两份表单独立保存
- **WHEN** 用户编辑 Vision Tools 表单的 modelType 并保存
- **THEN** 仅 Vision Tools 表单提交（`visionSave`），Image Bridge 表单的草稿不受影响；反之亦然

#### Scenario: inject 返回对象而非数组
- **WHEN** `plugins.bundle.config` 注册的 `inject()` 被调用
- **THEN** 返回值是对象，含 `remote`（dashboard Service）、`hooks.visionCard` / `hooks.bridgeCard`（observable stores）、`visionEdit` / `visionSave` / `bridgeEdit` / `bridgeSave` 等命名空间前缀方法；SHALL NOT 返回数组

#### Scenario: 账号池区块位于汇总之后表单之前
- **WHEN** 用户打开 bundle 详情页
- **THEN** 账号池区块出现在仪表盘汇总之后、Vision Tools 表单之前

#### Scenario: 账号池区块呈现未启用状态
- **WHEN** 账号池不存在或为空
- **THEN** 账号池区块呈现「未启用账号池」，其它区块与表单行为不受影响

#### Scenario: dashboard row 未启用时不抛错
- **WHEN** dashboard row 未启用（但某个 config entry 已启用），`ctx.remote.visionaryDashboard` 为 undefined
- **THEN** 插件 SHALL 跳过对 Remote 的调用，页面 SHALL NOT 抛错，其余区块与表单行为正常

#### Scenario: Remote 返回形状兼容
- **WHEN** Remote 调用返回 `{ok:true,value}` 包裹的结果，或返回裸值
- **THEN** 两种形状都能被正确解包，账号池数据正常渲染

#### Scenario: 添加账号为非阻塞启动语义
- **WHEN** 用户在账号池区块点击「添加账号」
- **THEN** host 侧启动登录子进程后立即返回，插件面板保持可交互并呈现「等待登录完成」，SHALL NOT 阻塞整个面板

### Requirement: 二进制状态与版本兼容性

仪表盘 SHALL 展示 `visionary-server` 二进制的解析路径、版本号、与插件 `COMPAT_MINOR` 的兼容性。二进制缺失时 SHALL 明确提示缺失并给出修复指引（设置 `binaryPath` 或确保 PATH 可见）。版本不兼容时 SHALL 标记不兼容并展示期望的 minor 范围。数据 SHALL 经 host Service RPC 实时查询（每次打开详情页刷新；不缓存跨会话）。

#### Scenario: 二进制就绪且兼容
- **WHEN** `visionary-server` 二进制解析成功，版本 `0.7.x` 与 `COMPAT_MINOR` "0.7" 匹配
- **THEN** 仪表盘二进制区块展示解析路径、版本号、兼容标记（绿色/就绪）

#### Scenario: 二进制缺失
- **WHEN** `resolveBinaryPath` 返回 null（PATH 不可见、`binaryPath` 空、`DEEPSEEK_VISIONARY_BIN` 未设）
- **THEN** 仪表盘二进制区块展示缺失状态，并给出修复指引（设置 binaryPath / 确保 PATH / 设 DEEPSEEK_VISIONARY_BIN）

#### Scenario: 版本不兼容
- **WHEN** 二进制解析成功但版本 minor 与 `COMPAT_MINOR` 不匹配（如二进制 0.6.x，插件期望 0.7）
- **THEN** 仪表盘二进制区块标记不兼容，展示实际版本与期望 minor 范围

### Requirement: 登录/会话状态

仪表盘 SHALL 展示 `deepseek_vision_status` 的关键结果：登录态（已登录/未登录/登录中）、PoW 状态、上传配额等运行时状态。查询 SHALL 经 host Service 调用 `visionary-server status --json`（复用现有 `runCli`），SHALL NOT 经 client 直接 spawn。查询失败（二进制缺失、CLI 报错、超时）时 SHALL 展示错误状态而非崩溃。

#### Scenario: 已登录
- **WHEN** `visionary-server status --json` 返回登录态有效
- **THEN** 仪表盘登录区块展示已登录，含 PoW/配额等运行时字段

#### Scenario: 未登录
- **WHEN** `visionary-server status --json` 返回未登录
- **THEN** 仪表盘登录区块展示未登录，并提示用 `deepseek_vision_login` 工具或诊断动作登录

#### Scenario: 状态查询失败
- **WHEN** `visionary-server status --json` 报错（二进制缺失/超时/CLI 异常）
- **THEN** 仪表盘登录区块展示错误状态（错误消息摘要），不崩溃整个仪表盘

### Requirement: 桥接运行态

仪表盘 SHALL 展示桥接当前配置（`enabled` / `scope` / `mode`）的运行态值与落盘副本数。运行态值 SHALL 经 `configForms` scope 读取（与 `plugins.item` bridge 卡同源，不重复声明）。落盘副本数 SHALL 经 host Service 查询 `pastedDir` 目录（复用 persistence 计数逻辑）。桥接未启用时 SHALL 展示禁用状态。

#### Scenario: 桥接启用且有落盘副本
- **WHEN** 桥接 `enabled=true`，`pastedDir` 下有 N 个副本
- **THEN** 仪表盘桥接区块展示 enabled/scope/mode 当前值与副本数 N

#### Scenario: 桥接禁用
- **WHEN** 桥接 `enabled=false`
- **THEN** 仪表盘桥接区块展示禁用状态，副本数不查询（或展示 N/A）

### Requirement: 诊断动作按钮

仪表盘 SHALL 提供以下诊断动作按钮，每个动作 SHALL 经 host Service RPC 执行，执行期间按钮展示 loading 状态，完成后刷新相关区块：

- **重探测二进制**：重新跑 `resolveBinaryPath`，刷新二进制区块
- **重新登录**：触发 `visionary-server login`（异步，不阻塞仪表盘），完成后刷新登录区块
- **清理落盘**：清理 `pastedDir` 下副本（复用 bridge 的 `cleanupAll` 逻辑），完成后刷新桥接区块副本数
- **自检**：跑一次 `visionary-server status --json` + 二进制版本检查，汇总结果

动作失败时 SHALL 在按钮处展示错误摘要，不影响其他区块。

#### Scenario: 重探测二进制
- **WHEN** 用户点击「重探测二进制」
- **THEN** 按钮进入 loading，host Service 重新跑 `resolveBinaryPath`，完成后二进制区块刷新（可能从缺失→就绪，若用户期间修复了 PATH）

#### Scenario: 清理落盘
- **WHEN** 用户点击「清理落盘」
- **THEN** 按钮进入 loading，host Service 清理 `pastedDir` 下副本，完成后桥接区块副本数刷新为 0（或剩余数）

#### Scenario: 动作失败不崩溃
- **WHEN** 诊断动作执行失败（如清理时目录不可写）
- **THEN** 该按钮处展示错误摘要，其他区块与按钮不受影响

### Requirement: host Service RPC 契约

仪表盘数据与动作 SHALL 经一个 cordis Service（`VisionaryDashboard`）暴露，client 经 `ctx.remote.visionaryDashboard` 调用。Service SHALL：

- 暴露 `getStatus()` 返回聚合状态（二进制/登录/桥接/副本数）
- 暴露 `reprobeBinary()` / `relogin()` / `cleanPasted()` / `selfCheck()` 四个动作方法
- 内部复用现有 `resolveBinaryPath` / `runCli` / persistence 逻辑，SHALL NOT 重写管道
- Service 缺席（如 host 未装 dashboard entry）时 client 仪表盘段 SHALL 不渲染（graceful degradation）

#### Scenario: client 经 RPC 拿状态
- **WHEN** 仪表盘段渲染且 host dashboard Service 在位
- **THEN** client 经 `ctx.remote.visionaryDashboard.getStatus()` 拿聚合状态，渲染 4 个区块

#### Scenario: Service 缺席时 graceful degradation
- **WHEN** host 未装 dashboard entry（如旧版本或被禁用）
- **THEN** client 仪表盘段不渲染（无 `ctx.remote.visionaryDashboard` 报错）

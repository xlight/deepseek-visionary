## ADDED Requirements

### Requirement: 图像分析工具
MCP 服务 SHALL 暴露 `deepseek_vision` 工具，接受本地图片路径或 base64/data URI 图片、可选 prompt、thinking 开关与会话续聊参数，返回视觉模型的文本分析结果。图片路径支持绝对路径；相对路径按服务器进程 cwd 解析（对齐 Python 行为）。

#### Scenario: 分析本地图片
- **WHEN** 用户调用 `deepseek_vision` 并传入存在的本地图片路径
- **THEN** 服务返回图片内容的文本描述

#### Scenario: 分析 base64 图片
- **WHEN** 用户以 base64 编码或 data URI 形式传入图片
- **THEN** 服务解码后完成分析并返回结果

#### Scenario: 图片文件不存在
- **WHEN** 传入的本地路径不存在
- **THEN** 服务返回明确的"图片未找到"错误信息

### Requirement: Vision 流水线
服务 SHALL 按"上传（含上传前 PoW 挑战）→ fork 到 vision 模型 → 获取 HIF 签名 → completion（含 completion 前 PoW 挑战）流式请求"的顺序执行完整流水线，并解析 SSE 流返回文本。

#### Scenario: 完整分析成功
- **WHEN** 上传、fork、签名与 completion 各步骤均成功
- **THEN** 返回视觉模型生成的完整文本

#### Scenario: PoW 挑战求解
- **WHEN** 服务在上传或调用 completion 前需要求解 PoW 挑战
- **THEN** 服务加载内置 WASM 求解器计算并通过校验

#### Scenario: 上传被 403 拦截
- **WHEN** 上传请求因 Cloudflare / TLS 指纹校验返回 403
- **THEN** 服务返回可诊断的错误信息并提示 cf_clearance / 重新登录

#### Scenario: TLS 指纹被拒
- **WHEN** completion 端点因 TLS 指纹校验返回 403
- **THEN** 服务返回可诊断的错误信息而非静默失败

### Requirement: 状态检查
MCP 服务 SHALL 暴露 `deepseek_vision_status` 工具，报告认证状态，并通过轻量鉴权接口真实校验 token 有效性。

#### Scenario: token 有效
- **WHEN** 已配置有效 token 且鉴权接口校验通过
- **THEN** 状态报告显示已认证

#### Scenario: token 失效
- **WHEN** token 缺失或鉴权校验失败
- **THEN** 状态报告提示需要执行登录

### Requirement: 会话续聊
服务 SHALL 将会话 id 与 parent_message_id 持久化到 `~/.deepseek-vision/session.json`，支持在同一会话内继续多轮对话，或通过显式 session_id 切换会话。

#### Scenario: 继续上次会话
- **WHEN** 用户以 continue_conversation 调用并存在持久化会话
- **THEN** 服务复用该会话与父消息链继续对话

#### Scenario: 显式切换会话
- **WHEN** 用户传入显式 session_id
- **THEN** 服务在该会话下发起新消息，并将结果持久化

#### Scenario: 显式会话无持久化记录
- **WHEN** 用户传入显式 session_id 但本地无对应持久化记录
- **THEN** 行为对齐 Python 版：仅复用该 session_id，不携带 parent_message_id

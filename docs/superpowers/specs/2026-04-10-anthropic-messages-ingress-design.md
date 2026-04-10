# AISIX Anthropic Messages Ingress 设计

## 背景

当前 AISIX 只对外暴露 OpenAI 兼容入口：`POST /v1/chat/completions` 和 `POST /v1/embeddings`。

在内部实现上，AISIX 已经具备按 provider kind 选择 codec、将上游响应归一化并回传给客户端的能力，因此已经支持“OpenAI 风格客户端请求 AISIX，再由 AISIX 访问 Anthropic 或 OpenAI-compatible 上游”的链路。

本次设计要补齐的是相反方向的能力：让客户端使用 Anthropic Messages API 访问 AISIX，而 AISIX 继续访问 OpenAI-compatible 上游（例如私有部署的 DeepSeek OpenAI 兼容服务）。

目标不是引入新的上游 provider，而是在现有 pipeline 上增加一层 Anthropic ingress / egress 协议兼容层。

## 范围

本次设计仅覆盖 Anthropic Messages API 兼容入口：`POST /v1/messages`。

本次必须支持：

- 非流式请求
- 流式请求
- 尽量兼容 Anthropic Messages 常用字段
- 将请求转换后转发给 OpenAI-compatible 上游
- 将 OpenAI JSON / SSE 响应转换为 Anthropic JSON / SSE 返回给客户端

本次明确不包含：

- Anthropic 风格 embeddings 入口
- Anthropic tool use / tool result 执行能力
- 多模态 content block（image、file、document 等）
- thinking / container / MCP / server tools
- 新增 provider kind
- 改造现有 Admin API 或 etcd schema 语义

## 目标

- 增加 `POST /v1/messages` 路由，使 Anthropic SDK 或 Anthropic 风格客户端可以直接接入 AISIX。
- 保持现有 `/v1/chat/completions` 与 `/v1/embeddings` 行为不变。
- 让 Anthropic ingress 能复用现有认证、授权、限流、缓存、路由选择和 usage 记录主链路。
- 为后续补充 tool use、多模态等能力保留清晰扩展边界。

## 非目标

- 不伪装支持尚未实现的 Anthropic 高级能力。
- 不为“尽量兼容字段”而静默忽略会导致语义错误的关键字段。
- 不在本次设计中重构所有 provider codec trait。
- 不改变当前缓存仅覆盖非流式 chat 的约束。
- 不改变当前 upstream provider 配置模型与 provider registry 的职责分工。

## 选定方向

采用“Anthropic ingress + richer canonical chat + OpenAI-compatible upstream mapping + Anthropic egress”的方向。

具体来说：

- 新增 `/v1/messages` handler，解析 Anthropic 请求。
- 扩展当前过于 OpenAI 化的 chat canonical 表达，使其能够承载 Anthropic Messages 常用语义。
- 继续沿用现有 `OpenAiCompatCodec` 访问 OpenAI-compatible 上游。
- 在 server 层增加 OpenAI JSON / SSE 到 Anthropic JSON / SSE 的响应适配。
- 对明确未实现的 Anthropic 能力返回 Anthropic 风格错误，而不是静默降级。

该方案不把 Anthropic 入口临时压扁成当前 `ChatRequest`，因为这种做法会过早丢失字段语义，无法满足“尽量兼容所有字段”的目标。

## 设计原则

### 1. 协议入口与上游 provider 能力分离

Anthropic ingress 是客户端协议兼容层，不等同于新增 Anthropic 上游 provider 行为。

也就是说：

- 客户端可以使用 Anthropic Messages API 访问 AISIX。
- AISIX 内部仍然可以把请求发往 OpenAI-compatible 上游。
- Anthropic ingress 与 provider kind `anthropic` 是两条独立维度，不能混为一谈。

### 2. Canonical request 必须保留语义，而不是只保留序列化后结果

如果仅在 handler 中把 Anthropic 请求临时转换成今天的 `ChatRequest`，那么 `system`、`stop_sequences`、多段文本 content、sampling 参数以及未来工具调用预留位都会被压缩或丢失。

因此，pipeline 内部需要一个比当前 `ChatRequest` 更强的 canonical chat 结构，用于承载跨协议共享的聊天语义。

### 3. 明确区分“已支持”“best-effort”“拒绝”三类字段

“尽量兼容所有字段”不意味着对任意字段都假装支持。

设计必须清晰规定：

- 哪些字段会进入 canonical 并被正式支持
- 哪些字段会被接收但只能 best-effort 映射到 OpenAI-compatible 上游
- 哪些字段首版必须返回明确错误

### 4. Anthropic 响应必须按客户端协议重建

如果客户端走 `/v1/messages`，则响应必须是 Anthropic 风格：

- 非流式返回 Anthropic message JSON
- 流式返回 Anthropic event stream
- 错误返回 Anthropic error envelope

不能把上游 OpenAI JSON / SSE 原样透给 Anthropic 客户端。

## API 设计

### 新增路由

在 `aisix-server` 中新增：

- `POST /v1/messages`

该路由由新的 Anthropic handler 负责。

### 请求头约定

Anthropic 客户端通常会发送 `anthropic-version`。为兼容常见 SDK，本次设计要求：

- 接收并解析 `anthropic-version`
- 若缺失该头，返回 Anthropic 风格 `invalid_request_error`
- 首版只接受一个固定版本值，或接受任意非空值但不在运行时分支语义，两者都可以；推荐先接受任意非空值并记录下来，避免不必要的 SDK 兼容阻塞

本次不引入真正的多版本行为分叉。

### 非流式响应

当 `stream = false` 时，AISIX 返回 Anthropic message 对象。

响应必须包含：

- `id`
- `type = "message"`
- `role = "assistant"`
- `model`
- `content`
- `stop_reason`
- `stop_sequence`
- `usage`

其中 `content` 首版只输出 text block。

### 流式响应

当 `stream = true` 时，AISIX 返回 Anthropic 风格 SSE。

事件序列采用稳定、可被 SDK 消费的最小集合：

- `message_start`
- `content_block_start`
- `content_block_delta`
- `message_delta`
- `message_stop`

不追求把 OpenAI token chunk 一比一映射回 Anthropic 内部所有流事件细节，但必须保证：

- Anthropic 客户端能持续消费内容增量
- 最终 stop 语义和 usage 语义可恢复
- 事件顺序始终合法

## 字段兼容策略

### 一等支持字段

以下字段必须进入 richer canonical chat 结构：

- `model`
- `messages`
- `system`
- `stream`
- `max_tokens`
- `stop_sequences`
- `temperature`
- `top_p`
- `metadata`
- `user`

这些字段要么直接影响上游请求，要么影响日志、审计、缓存或未来演进，不应只在 handler 层临时处理。

### Best-effort 字段

以下字段允许接收，但首版只做 best-effort：

- `top_k`
- `service_tier`
- 其他 provider-specific 扩展字段
- 未知但结构可保留的对象字段

处理原则：

- parser 接收这些字段
- richer canonical chat 或 ingress extras 保留原始值
- 若 OpenAI-compatible 上游存在清晰映射，则映射
- 若不存在清晰映射，则不送上游，但保留在 AISIX 内部上下文中
- 不伪造这些字段已经在上游生效的假象

### 明确拒绝字段

以下字段在首版必须返回明确错误：

- `tools`
- `tool_choice`
- `thinking`
- `container`
- MCP / server tools 相关字段
- 图片、文件、文档等非 text content block
- `tool_use` / `tool_result` content block

拒绝原因应回到 Anthropic 风格 `invalid_request_error`，并明确指出该字段在当前实现中不支持。

## Content 兼容策略

### 首版支持的消息内容

首版支持以下消息形态：

- `content` 为字符串
- `content` 为 text block 数组，例如 `[{"type":"text","text":"hello"}]`
- `role` 为 `user` 或 `assistant`

`system` 支持：

- 字符串
- 可拼接为单个系统提示的字符串数组 / text block 数组

### 首版不支持的消息内容

首版不支持：

- 图片 block
- 文件 block
- 文档 block
- `tool_use`
- `tool_result`
- 其他非 `text` block

当请求包含这些内容时，返回明确错误，而不是丢弃后继续请求上游。

## Canonical 数据模型

### 当前问题

当前 `aisix-types::request::ChatRequest` 结构过于偏向 OpenAI，只保留：

- `model`
- `messages: Vec<Value>`
- `stream`

这不足以承载 Anthropic ingress 所需字段。

### 建议方向

在 `aisix-types` 中引入更强的 canonical chat 表达，供 pipeline 内部使用。

建议结构至少表达：

- model name
- normalized messages
- normalized system prompt
- stream flag
- max tokens
- stop sequences
- sampling params
- metadata
- user
- ingress protocol family

是否保留原有 `ChatRequest` 作为 OpenAI ingress DTO 可以在实现计划中决定，但 pipeline 主线应转向 richer canonical chat，而不是继续直接依赖 OpenAI DTO。

## 组件拆分

### `aisix-types`

负责新增共享协议类型与 richer canonical request。

建议新增：

- Anthropic request / response / stream event 类型
- Anthropic error body 类型
- richer canonical chat 结构

这些结构属于跨 crate 共享契约，不应放在 `aisix-server` 私有模块里。

### `aisix-core`

负责扩展 `RequestContext`，使其能表达：

- ingress protocol
- egress protocol
- richer canonical chat request
- 未来可能需要的协议辅助元信息

目标是让 pipeline 能知道当前该按 OpenAI 还是 Anthropic 协议组装响应。

### `aisix-server`

负责：

- 新增 `POST /v1/messages` handler
- Anthropic request decode / validate
- Anthropic error rendering
- Anthropic JSON response rendering
- Anthropic SSE response rendering

并在现有 pipeline 出口增加“按客户端协议渲染响应”的分支。

### `aisix-providers`

继续负责与 OpenAI-compatible 上游通信。

首版不要求重写整个 `ProviderCodec` trait，只要求：

- `OpenAiCompatCodec` 能从 richer canonical chat 构建上游请求
- 现有 OpenAI JSON / SSE 归一化辅助层可被 Anthropic egress 复用

Anthropic ingress 不改变 `ProviderRegistry` 的职责：provider kind 仍然只描述上游 provider 类型。

## 请求数据流

新增 `/v1/messages` 后，请求主链路定义如下：

1. route match 到 `POST /v1/messages`
2. 解析并校验 Anthropic request body / headers
3. 转换为 richer canonical chat request
4. 初始化 `RequestContext`，标记 ingress / egress protocol 为 Anthropic
5. 复用现有 authentication、authorization、rate limit、cache、route select、upstream call、post-call usage 记录阶段
6. `OpenAiCompatCodec` 将 canonical request 转换成 OpenAI-compatible `/v1/chat/completions` 请求
7. 获取上游 OpenAI JSON / SSE 响应
8. 在 server 层将其转换成 Anthropic JSON / SSE
9. 回写 Anthropic 风格响应头、body、usage / stop 语义

这条链路的关键点是：pipeline 主体复用，协议适配分别发生在入口与出口。

## 上游请求映射

Anthropic request 到 OpenAI-compatible upstream 的首版映射规则如下：

- `model` -> 上游 model
- `system` -> 插入 OpenAI system message
- `messages` -> OpenAI messages
- `max_tokens` -> 上游 `max_tokens`
- `stop_sequences` -> 上游 `stop`
- `temperature` -> 上游 `temperature`
- `top_p` -> 上游 `top_p`
- `user` -> 上游 `user`
- `stream` -> 上游 `stream`

`metadata` 的处理：

- 如果上游支持 `metadata`，则透传
- 如果当前 OpenAI-compatible 上游不支持，则保留在 AISIX 上下文中，不强行送上游

`top_k` 的处理：

- 只有上游明确支持时才下发
- 否则只保留、不生效

## 响应映射

### 非流式映射

OpenAI non-stream chat completion 转 Anthropic message 的规则：

- `choices[0].message.role` 固定映射为 `assistant`
- `choices[0].message.content` 转为 `content: [{"type":"text","text":...}]`
- OpenAI `finish_reason` 映射到 Anthropic `stop_reason`
- OpenAI `usage.prompt_tokens` / `completion_tokens` 映射到 Anthropic `usage`
- `stop_sequence` 只有在可恢复时才填值，不能恢复时置空

### 流式映射

OpenAI SSE 转 Anthropic SSE 的规则：

- 首个有效 chunk 触发 `message_start`
- 同时发出一个 `content_block_start`
- 每个包含文本增量的 OpenAI delta 映射成 `content_block_delta`
- 当观察到 finish reason 或 usage 信息时，发出 `message_delta`
- 流结束时发出 `message_stop`

AISIX 不透传 OpenAI `[DONE]`，而是把它吸收为 Anthropic 终止事件。

如果上游流式响应中 usage 仅在尾帧出现，则 Anthropic `message_delta` 负责携带最终 usage / stop 元信息。

## 错误处理

### 协议要求

当客户端命中 `/v1/messages` 时，任何错误都应渲染为 Anthropic 风格错误对象，而不是当前默认的 OpenAI 风格错误对象。

这包括：

- 请求解码失败
- 缺少或非法的 `anthropic-version`
- 不支持字段
- 鉴权失败
- 权限不足
- 限流
- 上游失败
- 内部错误

### 错误分类

建议统一映射为 Anthropic 风格 error envelope，并按现有 `GatewayError::kind` 选择 status：

- `Authentication` -> 401
- `Permission` -> 403
- `InvalidRequest` -> 400
- `RateLimited` -> 429
- `Upstream` -> 502
- `Timeout` -> 504
- `Internal` -> 500

其中“字段已被识别但尚未支持”的场景必须使用 `invalid_request_error`，并在消息中明确指出不支持的字段名或 block type。

## 缓存与 usage

当前缓存仅覆盖非流式 chat。该约束在 Anthropic ingress 下保持不变。

也就是说：

- `POST /v1/messages` 非流式请求可以走现有 chat cache 语义
- `POST /v1/messages` 流式请求不缓存

usage 记录保持现有 post-call 模式：

- 成功响应后从 `RequestContext` 读取 usage
- Anthropic ingress 只改变客户端协议，不改变 usage 记录机制

## 测试策略

### 集成测试

新增 `/v1/messages` 端到端测试，至少覆盖：

- 非流式文本 happy path
- 流式文本 happy path
- `system + stop_sequences + max_tokens` 映射
- `content` 为字符串
- `content` 为 text block 数组
- 上游 OpenAI error 转 Anthropic error
- 缺少 `anthropic-version` 的错误

### 单元测试

新增协议映射层单元测试，覆盖：

- Anthropic request -> richer canonical chat
- richer canonical chat -> OpenAI upstream request
- OpenAI JSON -> Anthropic message
- OpenAI SSE -> Anthropic SSE events
- 不支持字段的错误分类

### 回归测试

必须保证以下现有路径不回归：

- `/v1/chat/completions` 非流式
- `/v1/chat/completions` 流式
- `/v1/embeddings`
- OpenAI upstream / Anthropic upstream 现有测试

## 风险与取舍

### 1. canonical chat 扩展会触及多处调用链

这是本次设计的主要改动面，但如果不做，Anthropic ingress 的字段兼容只能停留在浅层转换，后续会持续返工。

### 2. OpenAI SSE 与 Anthropic SSE 事件模型并不完全同构

首版采用“可消费、事件序列合法、usage 可恢复”的稳定映射，而不是追求逐 token 语义完全对齐。

### 3. 过度兼容会制造静默错误

因此本设计选择对 tool use、多模态、thinking 等能力明确报错，而不是假装接受。

## 实施边界

本设计支持把 Anthropic Messages API 作为 AISIX 的客户端入口协议之一，但不要求把整个系统重构成完全协议无关的通用接口层。

首版的成功标准是：

- Anthropic SDK 能通过 `POST /v1/messages` 访问 AISIX
- AISIX 能把请求转给 OpenAI-compatible 上游并返回 Anthropic 风格结果
- 常见文本对话字段有清晰兼容策略
- 现有 OpenAI 入口和已有 provider 行为不回归

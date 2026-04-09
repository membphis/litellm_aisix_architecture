# Handler 阶段直写式重构设计

## 背景

当前 `aisix` 的请求链路已经实现了部分目标 pipeline 能力，但主要编排仍集中在 `crates/aisix-server/src/pipeline.rs` 中的 `run_json_pipeline` 和 `run_chat_stream_pipeline`。这两个函数目前把阶段顺序、请求类型分支、缓存特判、路由解析、上游调用、响应重建、usage 记录都揉在同一个函数体内。

这种写法可以工作，但它不符合 `aisix-backend-architecture.md` 中 `#### Handler 入口` 所表达的可读性目标：请求路径应当能够直接读成一串按顺序执行的阶段。

本次重构的目标是结构性调整，而不是功能性扩展：在保持当前行为不变的前提下，让请求链路读起来更接近架构文档。

## 范围

本次重构仅覆盖当前两个业务 handler：

- `chat_completions`
- `embeddings`

本次重构不新增产品能力，只整理当前已经实现的行为。

## 目标

- 让 handler 主路径能够直接读成有序的阶段序列。
- 让代码结构更贴近架构文档中的固定阶段 pipeline 风格。
- 保持 chat JSON、chat streaming、embeddings 的现有行为不变。
- 建立清晰的 stage 边界，为后续增加 guardrail、pre-upstream 等阶段打基础。

## 非目标

- 不引入 `PreCall Guardrail`。
- 不引入 `PreUpstream`。
- 本次重构不引入共享的通用 `run_pipeline()`。
- 不新增 `responses`、`images`、`audio`、`realtime`、`mcp` 等新 API handler。
- 不改变 policy 语义、缓存语义、限流语义、provider 语义或 usage 统计语义。

## 选定方向

采用架构文档中“可读性优先”的 handler 风格来重构当前已经实现的两个 handler。

具体来说：

- `chat_completions` 直接按 handler 内的阶段顺序书写。
- `embeddings` 直接按 handler 内的阶段顺序书写。
- 共享能力下沉到 stage 模块，而不是重新抽成一个新的总编排函数。
- `pipeline.rs` 不再作为这两个 handler 的主要请求编排入口。

这个选择有意把“阶段顺序可读”放在“过早追求跨端点统一编排复用”之前。

## 设计原则

### 1. handler 持有可见的阶段顺序

每个 handler 都应直接表现出读者预期的请求路径顺序：

1. Decode
2. Authentication
3. Authorization
4. RateLimit
5. Cache Lookup（适用时）
6. RouteSelect
7. 上游调用
8. PostCall usage 记录

handler 自身应只体现编排顺序，而不是承载大块混杂实现细节。

### 2. stage 模块持有阶段行为

每个 stage 模块应只做一件清晰的事，并隐藏其内部实现细节。

例如：

- `authorization::check` 负责校验请求模型是否允许访问。
- `rate_limit::check` 负责承接当前 `precheck` 逻辑。
- `cache::lookup_chat` 负责 chat cache key 构造、查询和缓存响应构建。
- `route_select::resolve` 负责把当前快照中的目标路由加载到 context。
- `stream_chunk::proxy` 负责执行上游调用并重建响应。
- `post_call::record_success` 负责从 context 中读取 usage 并做记录。

### 3. RequestContext 成为 pipeline 主线

请求上下文应体现“逐阶段填充”的模型，而不是在一开始就要求所有字段都具备。

建议持有：

- `request`
- `key_meta`
- `resolved_target: Option<ResolvedTarget>`
- `resolved_provider_id: Option<String>`
- `usage: Option<Usage>`
- `response_cached: bool`

这样才能与架构文档中“上下文逐步富化”的思路保持一致。

## 代码组织建议

在 `crates/aisix-server/src/pipeline/` 下建立面向 stage 的模块结构：

- `context.rs`
- `authorization.rs`
- `rate_limit.rs`
- `cache.rs`
- `route_select.rs`
- `stream_chunk.rs`
- `post_call.rs`
- `mod.rs`

将现有逻辑从 `crates/aisix-server/src/pipeline.rs` 中迁移到这些模块中。

本次重构不需要一次性镜像架构文档中的所有未来阶段，只为当前已经实现的能力建立模块即可。

## Handler 形态

### chat_completions

`chat_completions` 应成为当前最可读、最能代表阶段顺序的主路径。

期望形态：

```rust
pub async fn chat_completions(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<ChatRequest>,
) -> Result<Response, GatewayError> {
    let mut ctx = RequestContext::new(CanonicalRequest::Chat(request), authenticated_key.meta);

    authorization::check(&ctx)?;
    rate_limit::check(&ctx, &state).await?;

    if let Some(response) = cache::lookup_chat(&mut ctx, &state).await? {
        post_call::record_success(&ctx, &state).await;
        return Ok(response);
    }

    route_select::resolve(&mut ctx, &state)?;
    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    post_call::record_success(&ctx, &state).await;

    Ok(response)
}
```

说明：

- Authentication 继续由 extractor 负责。
- Decode 继续由 axum 负责。
- Cache 在本次重构中仍然只用于 chat，因为这与当前行为一致。
- chat 的 streaming 与 non-streaming 差异尽量收敛在 stage 内部，而不是在 handler 层暴露成明显的分支噪音，除非实现上不可避免。

### embeddings

`embeddings` 应保持相同风格，但不走 chat 专属缓存阶段。

期望形态：

```rust
pub async fn embeddings(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, GatewayError> {
    let mut ctx = RequestContext::new(
        CanonicalRequest::Embeddings(request),
        authenticated_key.meta,
    );

    authorization::check(&ctx)?;
    rate_limit::check(&ctx, &state).await?;
    route_select::resolve(&mut ctx, &state)?;
    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    post_call::record_success(&ctx, &state).await;

    Ok(response)
}
```

## 各阶段职责

### authorization

封装现有 `ensure_model_allowed` 行为，不改变语义。

### rate_limit

封装现有 `RateLimitService::precheck` 调用，不改变语义。

该 stage 可以在内部按需要加载 snapshot 并获取当前 concurrency guard。本次重构不改变底层 limiter 的行为。

### cache

负责 chat 的缓存命中与写回逻辑。

职责包括：

- 构造 cache key
- 查询内存缓存
- 命中时设置 `ctx.response_cached = true`
- 命中时在有 usage 的情况下填充 `ctx.usage`
- 命中时直接返回已构建响应
- 为成功的非流式 chat 上游响应提供写回 helper

本次重构中，embeddings 不使用该 stage。

### route_select

负责从当前已加载的快照中解析模型目标并写入 context。

职责包括：

- 解析 `ResolvedTarget`
- 将选中的 provider 身份写入 context
- 避免在 handler 中重复书写路由解析逻辑

### stream_chunk

作为当前已实现端点的上游执行阶段。

尽管名称叫 `stream_chunk`，本模块在当前实现中会同时覆盖：

- 非流式 JSON 上游调用
- chat 流式上游调用

职责包括：

- 根据选中的 target 解析 provider codec
- 按请求 transport mode 分派 JSON 或 SSE 上游执行
- 重建响应头
- 填充 `ctx.usage`
- 在适用时为成功的非流式 chat 响应执行缓存写回

模块命名继续贴近架构文档的阶段术语，即使当前实现范围略大于名字表面含义。

### post_call

负责当前已经实现的 usage 记录逻辑。

职责包括：

- 从 context 中读取 usage
- 在没有 usage 时 no-op
- 按当前语义记录 success counter

本次重构不引入异步后台 `spawn`。实现应首先保持当前行为不变。后续可以再演进到架构文档所描述的异步 post-call 模型。

## 行为保持要求

本次重构必须保持当前已覆盖端点的所有外部可见行为不变。

包括但不限于：

- Authentication 拒绝行为
- Authorization 拒绝行为
- RateLimit 拒绝行为
- Chat cache 命中与未命中行为
- `x-aisix-cache-hit` 响应头行为
- `x-aisix-provider` 响应头行为
- Chat streaming 行为
- Embeddings 行为
- Response usage extension 附加行为
- 成功响应与缓存命中场景下的 usage counter 更新行为

## 错误处理

stage 抽离不能改变当前错误面。

规则：

- 现有 `GatewayError` 的 kind 和 message 应尽量保持不变，除非纯重构下无法做到完全字面一致。
- handler 仍然在第一个失败 stage 处立即短路返回。
- cache miss 仍然属于非错误控制流。

## 测试策略

使用现有 handler 和 pipeline 邻近测试作为回归保障。

主要覆盖：

- `auth_flow`
- `rate_limit`
- `chat_non_stream`
- `stream_chat`
- `embeddings`

仅在符号位置或模块路径变化时，按需调整测试。不要围绕新的行为模型重写现有测试。

只有在 stage 抽离后出现更容易独立验证的逻辑时，才补充聚焦型单元测试。

## 迁移步骤

1. 调整 `RequestContext`，使其适合逐阶段填充。
2. 创建 `pipeline/` stage 模块，并将现有逻辑迁入。
3. 重写 `chat_completions`，使其直接体现阶段顺序。
4. 重写 `embeddings`，使其直接体现阶段顺序。
5. 删除或显著缩减旧的 `pipeline.rs` 编排入口。
6. 运行相关测试并修复回归。

## 权衡

### 接受的权衡

- `chat_completions` 和 `embeddings` 之间存在少量重复是可以接受的，因为当前优先级是可读性。
- `stream_chunk` 的实际实现范围比名字表面更广，但该命名与架构文档术语保持连续性。
- `post_call` 在本次重构中继续保持同步执行，以避免把结构调整和语义变更混在一起。

### 明确拒绝的方案

- 现在就引入单一通用 `run_pipeline()`。这很可能重新引入条件分支式编排，削弱本次明确追求的可读性。
- 继续把编排集中保留在 `pipeline.rs` 中。这会延续当前的可读性问题。

## 成功标准

当满足以下条件时，本次重构算成功：

- `chat_completions` 和 `embeddings` 在阅读上都能明显呈现为有序 stage 列表。
- 现有行为保持不变。
- 旧的复杂编排不再堆在单个大请求路径函数中。
- 后续新增未实现阶段时，不需要再次推倒 handler 结构重来。

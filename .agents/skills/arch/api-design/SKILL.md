---
name: arch-api-design
description: AISIX API 接口设计、请求管线、Admin API 和错误格式约定
trigger:
  files:
    - "aisix/crates/aisix-server/src/handlers/**"
    - "aisix/crates/aisix-server/src/pipeline/**"
    - "aisix/crates/aisix-server/src/admin/**"
    - "aisix/crates/aisix-server/src/app.rs"
    - "aisix/crates/aisix-server/src/stream_proxy.rs"
    - "aisix/crates/aisix-auth/**"
    - "aisix/crates/aisix-types/src/error.rs"
    - "aisix/crates/aisix-providers/src/**"
    - "aisix/docs/admin-api.md"
  keywords:
    - "handler"
    - "pipeline"
    - "admin api"
    - "stream chunk"
    - "provider codec"
    - "transport mode"
    - "error response"
    - "openai compatible"
    - "sse"
    - "fallback"
    - "guardrail"
    - "post call"
priority: high
related:
  - arch-style
  - arch-data-model
  - arch-infra
---

# AISIX API 设计与请求管线

## 核心原则：OpenAI 兼容接口

所有代理端点接受和返回 OpenAI 兼容的 JSON/SSE 格式。
网关在内部处理不同 Provider 格式之间的转换。

## 代理 API 端点

| 端点 | Operation | TransportMode |
|------|-----------|---------------|
| `POST /v1/chat/completions` | ChatCompletions | `SseStream` 或 `Json` |
| `POST /v1/embeddings` | Embeddings | 仅 `Json` |

Phase 2+：`POST /v1/images/generations`、Audio 端点、`POST /v1/responses`。

## 固定 Pipeline 序列

所有代理请求遵循相同的固定阶段顺序。每个阶段查询编译快照；无配置 = no-op。
不使用动态阶段组合。

```
Decode → Authentication → Authorization → RateLimit → PreCall Guardrail
→ Cache Lookup → PreUpstream → RouteSelect → UpstreamHeaders → StreamChunk
→ PostCall
```

### Handler 模式

每个 handler 是薄包装：Decode + `run_pipeline()` 调用。

```rust
async fn chat_completions(
    State(state): State<AppState>,
    AuthenticatedKey(key_meta): AuthenticatedKey,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Response, GatewayError> {
    run_pipeline(state, key_meta, CanonicalRequest::Chat(body)).await
}
```

### run_pipeline 函数

```rust
async fn run_pipeline(
    state: AppState,
    key_meta: KeyMeta,
    request: CanonicalRequest,
) -> Result<Response, GatewayError> {
    let mut ctx = RequestContext::new(request, key_meta);
    authorization::check(&ctx, &state)?;
    rate_limit::check(&ctx, &state).await?;
    guardrail::pre_call(&ctx, &state).await?;
    if let Some(resp) = cache::lookup(&ctx, &state).await? {
        // 缓存命中：跳过上游调用和限流扣费，但仍记录日志
        tokio::spawn(post_call::run(ctx.into_post_call_context(), state));
        return Ok(resp);
    }
    pre_upstream::apply(&mut ctx, &state)?;
    route_select::resolve(&mut ctx, &state)?;
    upstream_headers::inject(&mut ctx, &state)?;
    let resp = stream_chunk::proxy(&mut ctx, &state).await?;
    tokio::spawn(post_call::run(ctx.into_post_call_context(), state));
    Ok(resp)
}
```

## Authentication（认证）

- 自定义 Axum extractor `AuthenticatedKey`，通过 `FromRequestParts` 实现
- 从 `Authorization` 头提取 `Bearer` token
- 在 `CompiledSnapshot.keys_by_token` 中 O(1) HashMap 查找
- 检查 `expires_at`；过期 key 返回 401
- 将 `KeyMeta` 注入 pipeline 上下文

## Authorization（鉴权）

- 检查已认证 key 的 `allowed_models` 是否包含请求的 model
- 使用 `*` 通配符允许所有模型
- 不在允许列表中返回 403 `permission_denied`

## StreamChunk（核心复杂度）

按 `TransportMode` 分发，而非按 `Operation` 分发：

```rust
pub async fn proxy(ctx: &mut RequestContext, state: &AppState) -> Result<Response, GatewayError> {
    match ctx.request.transport_mode() {
        TransportMode::SseStream    => proxy_sse_stream(ctx, state).await,
        TransportMode::Json         => proxy_json_response(ctx, state).await,
        TransportMode::BinaryStream => proxy_binary_stream(ctx, state).await,
    }
}
```

### SseStream 的三个技术难点

1. **帧边界解析**：SSE 事件可能跨多个 TCP 包，也可能一个包含多个事件。
   必须维护跨帧状态机。流式响应开始后必须零 panic。

2. **多 Provider 格式转码**：每个 Provider codec 将其原生格式（OpenAI SSE、
   Anthropic event/data、Gemini JSON lines、Bedrock 二进制 eventstream）
   转换为统一的 `StreamEvent`。`stream_chunk` 模块只看到 `StreamEvent`，
   不感知 Provider 特定格式。

3. **首字节前 fallback**：在第一个响应字节发送给客户端之前，错误（超时、5xx）
   可以触发透明切换到备用 Provider。首字节之后无法 fallback（HTTP 响应头已发出，
   状态码已确定）。必须精确追踪这个边界。

### Cancel Safety

客户端断开 → axum drop SSE body future → 每个 `await` 点都可能被取消。
所有资源必须使用 RAII guard（Drop 实现），而非手动清理。
并发 guard 必须在 Drop 中 spawn 异步 ZREM 任务以释放 Redis 槽位。

## Provider Codec 接口

```rust
#[async_trait]
pub trait ProviderCodec: Send + Sync + 'static {
    fn kind(&self) -> ProviderKind;
    fn capabilities(&self) -> ProviderCapabilities;
    fn build_request(&self, ctx: &RequestContext, target: &ResolvedTarget)
        -> Result<http::Request<Body>, GatewayError>;
    async fn parse_response(&self, ctx: &RequestContext, target: &ResolvedTarget,
        resp: http::Response<Incoming>) -> Result<ProviderOutput, GatewayError>;
    fn normalize_error(&self, status: StatusCode, body: &[u8]) -> GatewayError;
}
```

### ProviderOutput

```rust
pub enum ProviderOutput {
    Json { status, headers, body: Bytes, usage: Option<Usage> },
    EventStream { status, headers, stream: Pin<Box<dyn Stream<Item = Result<StreamEvent, GatewayError>>>> },
    ByteStream { status, headers, stream: Pin<Box<dyn Stream<Item = Result<Bytes, GatewayError>>>> },
}
```

### Codec 复用策略

- `OpenAICompatCodec`：通用实现，覆盖所有 OpenAI 兼容 Provider
  （OpenAI、Azure、Ollama、vLLM、Groq）— 通过 base_url + auth 策略参数化
- 非兼容 Provider（Anthropic、Vertex、Bedrock）：各自独立实现
- 统一以 `Arc<dyn ProviderCodec>` 持有在 `ProviderRegistry` 中

## PostCall（异步、不阻塞响应）

- 在 `tokio::spawn` 中运行 — 不阻塞响应
- 接收 owned 的 `PostCallContext`（String、u64、bool — 不借用 ctx）
- 职责：Redis 用量记账、费用追踪、结构化日志
- 通过 structlog + callback sink 输出 `UsageEvent`

## Admin API

### 认证

所有 admin 请求需要 `x-admin-key` 头与配置值匹配。缺失或无效 → 401。

### 路由（每个集合：providers、models、apikeys、policies）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/admin/<collection>` | 列出所有（按 id 升序） |
| `GET` | `/admin/<collection>/:id` | 获取单个 |
| `PUT` | `/admin/<collection>/:id` | Upsert（路径 id 必须与 body 匹配） |
| `DELETE` | `/admin/<collection>/:id` | 删除 |

### 写入语义

Admin 写入先到 **etcd**。后台 watcher 异步应用变更。
成功的 PUT/DELETE 意味着 etcd 接受了写入，并不代表运行时快照已更新。

写入响应格式：
```json
{ "id": "openai", "path": "/aisix/providers/openai", "revision": 123 }
```

允许乱序写入（例如先写 model 再写其 provider）。收敛在 watcher 层完成。

### 错误码

| HTTP | 条件 |
|------|------|
| 401 | 缺失/无效 admin key |
| 400 | 无效 id 或路径/body 不匹配 |
| 404 | 资源不存在（GET/DELETE） |
| 500 | etcd 或服务器故障 |

## 错误响应格式

所有代理错误遵循 OpenAI 兼容 JSON：

```json
{
  "error": {
    "message": "Invalid API key provided",
    "type": "authentication_error",
    "code": "invalid_api_key"
  }
}
```

### ErrorKind → HTTP 状态码 + error.type 映射

| HTTP | `error.type` | ErrorKind |
|------|-------------|-----------|
| 401 | `authentication_error` | `Authentication` |
| 403 | `permission_denied` | `Permission` |
| 429 | `rate_limit_error` | `RateLimited` |
| 429 | `budget_exceeded` | （费用限制） |
| 400 | `invalid_request_error` | `InvalidRequest` |
| 502 | `upstream_error` | `Upstream` |
| 504 | `timeout_error` | `Timeout` |
| 500 | `internal_error` | `Internal` |

### 可重试错误

`ErrorKind::retryable()` 对以下返回 true：`RateLimited`、`Timeout`、
`UpstreamUnavailable`、`Overloaded`。重试/fallback 决策基于此方法，
而非 Provider 特定的错误字符串。

## 健康检查端点

| 端点 | 用途 | 成功 | 失败 |
|------|------|------|------|
| `GET /health` | 存活探针 | 200 | — |
| `GET /ready` | 就绪探针 | 200 | 503 |

`/ready` 检查：快照已加载 + Redis PING。

## 多端点 Pipeline 复用

| 组别 | API | 复用度 | 备注 |
|------|-----|-------|------|
| A | Chat、Responses | ~90% | SSE + 非流式 |
| B | Embeddings、Images | ~70% | 仅 JSON |
| C | Audio | ~55% | Multipart/二进制 |
| D | Realtime、MCP | ~20-30% | 不同协议 |

A+B 组共享 `run_pipeline()`。差异通过 `CanonicalRequest` 枚举和
`ProviderCodec` trait 多态消化。

## 当前阶段妥协

- [!NOTE] 尚无 guardrail 回调（Phase 3）。Pipeline 阶段以 no-op 存在。
- [!NOTE] 尚无 prompt template / 请求变更（Phase 2）。Pipeline 阶段以 no-op 存在。
- [!NOTE] Admin API 读取 key 时返回明文 `key` 字段。无哈希处理。
- [!NOTE] 尚无 fallback 机制（Phase 2）。RouteSelect 选择单一目标。
  首字节前 fallback 基础设施将随流式核心一起构建。

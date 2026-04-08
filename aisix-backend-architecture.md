# AISIX 后端架构设计方案

> **定位**：AISIX 是一个开源的高性能 AI Gateway，使用 Rust 开发，专注于数据面（data plane）能力。
>
> **命名灵感**：Apache APISIX —— 采用其数据面/控制面分离理念，但面向 AI/LLM 场景重新设计。
>
> **参考基准**：基于 [litellm-data-plane-panorama-20260403.md](./litellm-data-plane-panorama-20260403.md) 中 LiteLLM 的 14 大功能域进行对标设计。
>
> **文档版本**：2026-04-03

---

## 一、架构核心决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| **运行时** | tokio 多线程 | Rust 异步事实标准，MPMC 调度器，高并发流式友好 |
| **HTTP 框架** | axum + tower + hyper | Tower 中间件生态最好，hyper 底层零拷贝流式支持 |
| **架构模式** | 数据面/控制面硬分离 | 学习 APISIX 核心经验：请求路径不混入配置管理 |
| **配置同步** | etcd 协议（标准 etcd 集群） | 数据面只依赖 etcd 协议，watch 语义可靠，MVCC 天然保证一致性 |
| **分发策略** | Provider 用 `Arc<dyn ProviderCodec>`，扩展点用 HTTP callback | 可读性优先；Provider 列表有限且固定，dyn dispatch 开销在 AI 请求秒级延迟前可忽略 |
| **状态模型** | 不可变编译快照 + ArcSwap 原子切换 | 配置热加载零停机，无读写锁竞争 |
| **流式代理** | 统一解析转发 | 所有上游格式统一解析为 StreamEvent，再渲染为 OpenAI SSE；无需维护两条代码路径 |
| **Guardrails** | 内置 HTTP callback 引擎 | 不引入插件系统，guardrail 即外部 HTTP 服务调用 |
| **扩展方式** | 内置服务 + 外部 HTTP callback | 编译时类型安全，不牺牲性能 |

---

## 二、总体架构图

### 配置同步

```
  Control Plane ──▶ etcd Cluster ─────────────▶ AISIX Data Plane
                                               (etcd watch)
```

### 整体架构

```
      ┌──────────────────────────────────────────────┐
      │               Control Plane                  │
      │      CLI / Admin API / Dashboard             │
      └─────────────────────┬────────────────────────┘
                            │
                 ┌──────────┴──────────────┐
                 │      etcd Cluster       │
                 │    (source of truth)    │
                 └──────────┬──────────────┘
                            │ etcd watch
         ┌──────────────────┼──────────────────┐
         │                  │                  │
         ▼                  ▼                  ▼
 ┌───────────────┐  ┌───────────────┐  ┌───────────────┐
 │    Node A     │  │    Node B     │  │    Node C     │
 │ aisix-gateway │  │ aisix-gateway │  │ aisix-gateway │
 │               │  │               │  │               │
 │ etcd watcher  │  │ etcd watcher  │  │ etcd watcher  │
 │      ↓        │  │      ↓        │  │      ↓        │
 │   Compiled    │  │   Compiled    │  │   Compiled    │
 │   Snapshot    │  │   Snapshot    │  │   Snapshot    │
 └───────────────┘  └───────────────┘  └───────────────┘
```

### 数据面节点内部结构

```
┌──────────────────────────────────────────────────────────────┐
│                    AISIX Data Plane Node                     │
│                                                              │
│  ┌────────────────┐   ┌──────────────────────────────┐       │
│  │ etcd Watcher   │──▶│ Arc<CompiledSnapshot>        │       │
│  │ (config sync)  │   │ (immutable, ArcSwap atomic)  │       │
│  └────────────────┘   └──────────────┬───────────────┘       │
│                                      │                       │
│  ┌───────────────────────────────────▼────────────────────┐  │
│  │                Request Pipeline (axum/tower)           │  │
│  │                                                        │  │
│  │  Decode → Authz → Mutation → RateLimit → PreCall Guard │  │
│  │  → Cache → Router → Provider → Upstream → PostCall     │  │
│  │  Guard → Spend → Logging                               │  │
│  └──────────────────────────┬─────────────────────────────┘  │
│                             │                                │
│  ┌──────────────────────────▼─────────────────────────────┐  │
│  │           Upstream Pool (reqwest client)               │  │
│  │  pool: scheme+host+port, HTTP/2, keepalive             │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌─────────────────┐  ┌───────────────────────────────────┐  │
│  │ Redis Client    │  │ Background Tasks                  │  │
│  │ (rate/cache/cc) │  │ - emit UsageEvent (structlog)     │  │
│  └─────────────────┘  │ - health chk / metrics            │  │
│                        └──────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### 为什么选择 etcd 协议

| 优势 | 说明 |
|------|------|
| **单一协议** | 数据面只对接 etcd，无需同时维护多个存储客户端 |
| **Watch 语义可靠** | etcd watch 原生支持，revision 保证不丢事件 |
| **MVCC 一致性** | 多节点部署下配置一致性天然保证，无需自行实现 |
| **配置回滚** | etcd 历史版本天然支持，无需额外实现 |
| **APISIX 生态兼容** | 天然兼容 APISIX 控制面生态 |

### 为何不引入 Plugin / Pipeline 系统

#### 当前决策：无 plugin，无 pipeline

第一阶段目标是**功能够用、快速试错验证**，代码允许在后续阶段被完全重构。
在这个前提下，引入 pipeline 或 plugin 系统会带来额外复杂度，与目标相悖。

具体理由：

**1. 当前需求固定明确，不存在个性化扩展诉求**

目前已知的所有 AI Gateway 功能需求均是固定且明确的。调研友商实现可以发现：
虽然底层可能存在某种通用抽象，但最终暴露给用户的功能点全都是确定的、
面向所有用户的，而非为单个企业个性化定制。这说明当前阶段不需要 plugin 机制。

**2. 友商（如 LiteLLM）hook 实现方式存在明显问题**

现有 AI Gateway 产品（如 LiteLLM）大量使用 hook，但这些 hook 普遍挂在"HTTP 请求前/后"，
而非 AI Gateway 原生语义的阶段（如认证前后、鉴权前后、Guardrail 前后等）。
按 HTTP 执行阶段方式挂 hook，会导致实现代码晦涩难懂，无论对开发者还是维护者
都造成理解负担，且实现本身也相当困难。

**3. 现阶段无法做出正确的阶段抽象**

认证与鉴权应当合并为一个阶段，还是拆分成两个独立阶段？
各阶段内部通过优先级排序，还是在外部显式编排？
这些问题需要与大量用户功能、真实生产实践迭代后才能得出合理答案。
项目初期尚未积累足够的用户迭代，过早抽象反而会固化错误的边界。

#### 时间节点

预计 **1-2 个季度后**，随着核心功能趋于稳定、用户迭代逐步积累，
再系统考虑基础能力的通用抽象（如 pipeline 流水线或通用插件能力）。

#### 未来演进：开放问题

当以下条件满足时，开始评估是否引入 plugin/pipeline：
- 某些功能已确认高度稳定，边界清晰
- 积累了足够的生产用户迭代，对各阶段语义有明确共识

届时面临的核心选择：**局部改造**（仅对稳定阶段引入 plugin 接口）还是
**全面改造**（将整个请求链路重构为可配置的 stage pipeline）——
这个问题目前保持开放，待生产经验积累后再做决策。

当前代码刻意保持"无抽象、顺序调用"的风格（见 `run_pipeline`），
正是为将来的改造保留清晰的边界，而非被过早的抽象所绑架。

### 控制面（aisix-admin）

**aisix-admin** 是独立服务，与 aisix-gateway 完全分离：

| 维度 | aisix-admin（控制面） | aisix-gateway（数据面） |
|------|----------------------|-----------------|
| **职责** | 写 etcd / PostgreSQL；消费 UsageEvent 写入 PG | 只读 etcd；写结构化日志（UsageEvent） |
| **API** | Admin REST API（CRUD 配置实体） | LLM Proxy API（/v1/...） |
| **LLM 流量** | 不处理 | 全部处理 |
| **通信** | 写入 etcd | Watch etcd 变更 |

两者通过 etcd 解耦：aisix-admin 写，aisix-gateway 读，互不直接调用。

aisix-admin 管理的实体包括：Provider 配置、Virtual Key、Team/Member、Model、限流策略、Guardrail 规则等。MVP 阶段可以直接通过 `etcdctl` 或 Admin API 写入；Dashboard 为可选扩展。

---



### 设计原则

**可读性优先**：Provider 数量有限且固定（MVP 为 6 个），用 `Arc<dyn ProviderCodec>` 持有和调用，代码简洁易懂。AI 请求延迟以秒计，网关自身的 dyn dispatch 开销可忽略不计。

扩展点（guardrail、callback）用外部 HTTP callback，无需动态插件系统。

### 核心类型

```rust
// ===== aisix-types =====

pub enum Operation {
    ChatCompletions,
    Responses,
    Embeddings,
    Images,
    AudioTranscription,
    AudioSpeech,
    RealtimeSession,
    McpCall,
}

pub enum CanonicalRequest {
    Chat(ChatRequest),
    Responses(ResponsesRequest),
    Embeddings(EmbeddingsRequest),
    Images(ImageRequest),
    Audio(AudioRequest),
    Mcp(McpRequest),
}

/// 客户端期待的响应传输方式。
/// 由 Operation + stream 字段决定，与上游 Provider 无关。
/// 定义在 aisix-types 中，作为 CanonicalRequest 的内置固定能力。
pub enum TransportMode {
    SseStream,      // SSE 帧流式（Chat/Responses + stream:true）
    Json,           // 完整 JSON 响应
    BinaryStream,   // 二进制流（如 AudioSpeech 返回音频数据）
}

impl CanonicalRequest {
    /// 根据端点类型和请求参数，确定客户端期待的传输方式。
    /// stream_chunk 模块只关心此方法的返回值，不感知 Operation。
    /// 新增端点时只需在此方法中添加一行，stream_chunk 本身无需改动。
    fn transport_mode(&self) -> TransportMode {
        match self {
            Self::Chat(r) if r.stream => TransportMode::SseStream,
            Self::Responses(r) if r.stream => TransportMode::SseStream,
            Self::Audio(AudioRequest::Speech(_)) => TransportMode::BinaryStream,
            _ => TransportMode::Json,  // Embeddings, Images, AudioTranscription, 非流式 Chat/Responses
        }
    }
}

pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: bool,
    pub cache_write: bool,
    pub cost_micros_usd: u64,
}
```

### Virtual Key 元数据（KeyMeta）

`KeyMeta` 在 Authentication 阶段从 etcd 快照中按 key hash 加载，注入 `RequestContext`。它只保存**身份标识**；具体配置（限速阈值、guardrail 规则、缓存策略、预算等）保留在快照中，各 pipeline stage 按 id 按需查找，自行决定是否有实际操作。

```rust
// ===== aisix-types =====

/// Virtual Key 元数据：从 etcd 快照中加载，Authentication 后注入 RequestContext
/// 只保存身份标识；各阶段所需的具体配置（限速阈值、guardrail 规则、缓存策略等）
/// 保留在快照中，各 pipeline stage 按 id 按需查找，自行决定是否有实际操作
pub struct KeyMeta {
    // ── 身份标识（供各 stage 按 id 查快照）──────────
    pub key_id:      String,          // etcd 中 apikey 记录的 ID（如 "key-abc123"），用于查快照
    pub user_id:     Option<String>,
    pub customer_id: Option<String>,  // 最终用户标识（x-litellm-end-user）

    // ── 元信息 ────────────────────────────────────────
    pub alias:      Option<String>,   // 用户自定义备注名
    pub expires_at: Option<DateTime>, // None 表示永不过期
}
```

### Provider 编解码器

```rust
// ===== aisix-providers =====

/// 每个 LLM Provider 实现此 trait，负责请求/响应格式转换
#[async_trait]
pub trait ProviderCodec: Send + Sync + 'static {
    fn kind(&self) -> ProviderKind;
    fn capabilities(&self) -> ProviderCapabilities;

    /// 将 CanonicalRequest 构建为上游 HTTP 请求
    fn build_request(
        &self,
        ctx: &RequestContext,
        target: &ResolvedTarget,
    ) -> Result<http::Request<Body>, GatewayError>;

    /// 解析上游 HTTP 响应为统一输出
    async fn parse_response(
        &self,
        ctx: &RequestContext,
        target: &ResolvedTarget,
        resp: http::Response<Incoming>,
    ) -> Result<ProviderOutput, GatewayError>;

    /// 错误归一化
    fn normalize_error(&self, status: StatusCode, body: &[u8]) -> GatewayError;

    /// 发送请求（默认实现）：build_request → 上游 HTTP 发送 → parse_response
    /// 各 codec 通常无需覆写此方法；仅在需要自定义重试或特殊错误处理时才覆写。
    async fn execute(
        &self,
        ctx: &RequestContext,
        target: &ResolvedTarget,
        client: &reqwest::Client,
    ) -> Result<ProviderOutput, GatewayError> {
        let req = self.build_request(ctx, target)?;
        let raw_resp = client.send(req).await
            .map_err(|e| GatewayError::upstream_io(e))?;
        if !raw_resp.status().is_success() {
            let (parts, body) = raw_resp.into_parts();
            let bytes = collect_body(body).await.unwrap_or_default();
            return Err(self.normalize_error(parts.status, &bytes));
        }
        self.parse_response(ctx, target, raw_resp).await
    }
}

pub enum ProviderOutput {
    Json {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
        usage: Option<Usage>,
    },
    EventStream {
        status: StatusCode,
        headers: HeaderMap,
        stream: Pin<Box<dyn Stream<Item = Result<StreamEvent, GatewayError>> + Send>>,
        // 流的每个单元是有语义的 StreamEvent（Delta / Usage / Done）。
        // codec 负责将上游各种格式（OpenAI SSE、Anthropic event/data、
        // Gemini JSON lines、Bedrock binary eventstream）统一解析为 StreamEvent。
        // stream_chunk 逐事件渲染成 OpenAI SSE 帧并转发给客户端。
        // 遇到 StreamEvent::Usage 时将 token 累计值写入 ctx.usage。
        // PostCall 从 ctx.usage 读取计费数据，而非从 ProviderOutput 读取。
    },
    ByteStream {
        status: StatusCode,
        headers: HeaderMap,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, GatewayError>> + Send>>,
        // 流的每个单元是无语义的原始字节（如 AudioSpeech 返回的 MP3/Opus 数据）。
        // codec 不解析内容，直接将上游字节流透传出去。
        // stream_chunk 直接转发字节给客户端，不经过 StreamEvent 解析。
        // usage 在请求前已知（按字符数计算），不在流内累积。
    },
}
```

### Provider 分发

所有 Provider 统一用 `Arc<dyn ProviderCodec>` 持有，运行时通过 trait 方法调用：

```rust
/// CompiledSnapshot 中的 Provider 注册表
pub struct ProviderRegistry {
    /// provider_id → codec 实例
    pub codecs: HashMap<String, Arc<dyn ProviderCodec>>,
}

/// 使用示例：路由选定 target 后，直接拿 codec 执行
let codec = registry.codecs.get(&target.provider_id)?;
let output = codec.execute(ctx, &target, &upstream_client).await?;
```

`OpenAICompatCodec` 是一个通用实现，覆盖所有 OpenAI 兼容 Provider（OpenAI、Azure OpenAI、Ollama、vLLM、Groq 等），只需在注册时传入不同的 base URL 和 auth 策略即可复用。非兼容 Provider（Anthropic、Vertex AI、Bedrock）各自独立实现 `ProviderCodec`。

非兼容 Provider 实现示例（以 Anthropic 为例）：

```rust
pub struct AnthropicCodec {
    api_key: String,
}

#[async_trait]
impl ProviderCodec for AnthropicCodec {
    fn kind(&self) -> ProviderKind { ProviderKind::Anthropic }
    fn capabilities(&self) -> ProviderCapabilities { /* chat, streaming, vision, ... */ }

    fn build_request(
        &self,
        ctx: &RequestContext,
        target: &ResolvedTarget,
    ) -> Result<http::Request<Body>, GatewayError> {
        // CanonicalRequest → Anthropic Messages API 格式
        // 注入 x-api-key 头
    }

    async fn parse_response(
        &self,
        ctx: &RequestContext,
        target: &ResolvedTarget,
        resp: http::Response<Incoming>,
    ) -> Result<ProviderOutput, GatewayError> {
        // Anthropic event:/data: SSE → StreamEvent → ProviderOutput::Stream
    }

    fn normalize_error(&self, status: StatusCode, body: &[u8]) -> GatewayError {
        // Anthropic 错误码 → GatewayError 统一分类
    }
}
```

### 路由策略

```rust
pub trait RouterStrategy: Send + Sync + 'static {
    fn select<'a>(
        &self,
        candidates: &'a [RouteCandidate],
        ctx: &RouteContext,
        stats: &RouteStatsView,
    ) -> Result<&'a RouteCandidate, GatewayError>;
}

// 内置策略
// - SimpleShuffle: 基于 tpm/rpm 权重随机
// - LeastBusy: 选当前最空闲的 deployment
// - LatencyBased: 基于 EWMA 延迟选择最快
// - UsageBased: 基于使用量均衡分配
```

### 限流器

```rust
#[async_trait]
pub trait RateLimiter: Send + Sync + 'static {
    /// 请求前检查（预估 token）
    async fn precheck(
        &self,
        ctx: &RequestContext,
        limits: &ResolvedLimits,
    ) -> Result<RateDecision, GatewayError>;

    /// 请求后结算（实际 usage）
    async fn settle(
        &self,
        ctx: &RequestContext,
        usage: &Usage,
    ) -> Result<(), GatewayError>;
}
```

### 缓存后端

```rust
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    async fn get(&self, key: &CacheKey) -> Result<Option<CachedResponse>, GatewayError>;
    async fn put(&self, key: CacheKey, value: CachedResponse, ttl: Duration) -> Result<(), GatewayError>;
}
```

---

## 四、请求处理管线

### 4.1 完整请求路径

```
Client Request
  │
  ▼
[1. Route Match] ─── axum route matching: /v1/chat/completions, /v1/embeddings, ...
  │
  ▼
[2. Decode + Normalize] ─── deserialize into CanonicalRequest (unified internal type)
  │
  ▼
[3. RequestContext Init] ─── request_id + trace span + RequestContext object
  │
  ▼
[4. Authentication] ─── Virtual Key（Bearer token）
  │
  ▼
[5. Authorization] ─── resolve Key→Customer hierarchy, determine effective policy
  │                ─── allowed models, labels, params, limits
  │
  ▼
[6. Request Mutation] ─── apply prompt template
  │                   ─── drop params / modify params
  │                   ─── enforce user param / size check
  │
  ▼
[7. Rate Limit + Budget Precheck]
  │   ─── local shadow rate limiter (fast-reject obvious overages)
  │   ─── Redis authoritative rate limiter (RPM/TPM/concurrency/budget)
  │   ─── TPM: check remaining quota > 0 → allow; deduct actual tokens after response
  │
  ▼
[8. Pre-Call Guardrails] ─── concurrent HTTP callbacks (PII masking, content safety)
  │                      ─── can block / transform / annotate request
  │
  ▼
[9. Cache Lookup] ─── memory/Redis cache hit?
  │
  ├── hit ──▶ [normalize response] ──▶ [return cached response]
  │
  ▼ miss
[10. Routing] ─── look up ModelConfig by model name → determine provider
  │           ─── generate fallback plan (backup model list)
  │           ─── exclude cooled-down providers
  │
  ▼
[11. Provider Request Build] ─── codec builds upstream HTTP request
  │
  ▼
[12. Upstream Call] ─── timeout control
  │                 ─── retryable / fallback before first byte
  │
  ├── TransportMode::Json ────────────────────────────────────────────┐
  │    ▼                                                              │
  │  [ProviderOutput::Json: parse full response body]                 │
  │    ▼                                                              │
  │  [Post-Call Guardrails] ─── HTTP callback                         │
  │    ▼                                                              │
  │  [extract Usage/Cost]                                             │
  │    ▼                                                              │
  │  [cache write (optional)]                                         │
  │    ▼                                                              │
  │  [return JSON response]                                           │
  │    ▼                                                              │
  │  [async Spend/Logging] ─── emit UsageEvent to structlog + cb sink │
  │                                                                   │
  ├── TransportMode::SseStream ───────────────────────────────────────┐
  │    ▼                                                              │
  │  [ProviderOutput::EventStream: codec → StreamEvent stream]        │
  │    ▼      (all upstream formats unified: SSE / JSON-lines /       │
  │            Anthropic event/data / Bedrock binary eventstream)     │
  │  [During-Stream Guardrails] ─── timeout-only HTTP callback        │
  │    ▼                                                              │
  │  [incremental Usage tracking] ─── on StreamEvent::Usage:          │
  │       accumulate tokens → write ctx.usage                         │
  │    ▼                                                              │
  │  [render StreamEvent → OpenAI SSE → client]                       │
  │    ▼                                                              │
  │  [stream end → async settle/log]                                  │
  │                                                                   │
  └── TransportMode::BinaryStream ────────────────────────────────────┐
       ▼                                                              │
      [ProviderOutput::ByteStream: raw bytes, no parsing]             │
       ▼                                                              │
      [forward bytes directly to client]                              │
       ▼                                                              │
      [async Spend/Logging] ─── usage known before request (char count)│
```

---

### 4.2 技术分层：Tower vs axum Handler

请求进入 AISIX 后，经过两个不同性质的代码层：

```
┌───────────────────────────────────────────────────────────────┐
│                    Tower Middleware Stack                     │
│  (global scope; any layer may return a response directly)     │
│                                                               │
│  ┌─────────────────────────────────────────────────────┐      │
│  │  RequestBodyLimitLayer ← tower-http, max body size  │      │
│  ├─────────────────────────────────────────────────────┤      │
│  │  TraceLayer            ← tower-http, auto span/log  │      │
│  └─────────────────────────────────────────────────────┘      │
└───────────────────────────────────────────────────────────────┘
                              │
                              ▼ enters after route match
┌───────────────────────────────────────────────────────────────┐
│                     axum Handler layer                        │
│  (sequential execution, shared State, early return via `?`)   │
│                                                               │
│  1. Decode          deserialize body + extract Extractor      │
│  2. Authentication  validate API Key → resolve tenant/key meta│
│  3. Authorization   check if key may access model/operation   │
│  4. RateLimit       check remaining quota > 0; deduct actual  │
│                     tokens after response                     │
│  5. PreCall Guard   call external guardrail HTTP service (opt)│
│  6. CacheLookup     semantic cache check, short-circuit on hit│
│  7. RouteSelect     pick upstream + load-balance + fallback   │
│  8. PreUpstream     inject prompt + replace vars + override   │
│  9. UpstreamHeaders assemble upstream auth headers, Host      │
│  10. StreamChunk    send request + transcode stream + forward │
│  11. PostCall       spawn(PostCallContext): cache, usage, log │
│  12. OnError        classify error → error response + fallback│
└───────────────────────────────────────────────────────────────┘
```

**关键区别**：
- Tower Layer 写一次挂上去就生效，不需要在每个 handler 里重复逻辑
- axum Handler 内部是普通 async Rust 代码，按顺序调用，用 `?` 早返回
- 两层之间没有魔法，边界非常清晰

---

### 4.3 各阶段实现分类

| # | 阶段 | 实现方式 | 分类 |
|---|------|---------|------|
| 1 | **Body 大小限制** | `tower_http::limit::RequestBodyLimitLayer`，限制最大请求体积，超限自动 413 | ✅ 社区 crate |
| 2 | **链路追踪** | `tower_http::trace::TraceLayer` + `tracing` crate | ✅ 社区 crate |
| 3 | **Decode** | `axum::Json<T>` Extractor 自动反序列化；自定义 `FromRequest` 处理 OpenAI 多 endpoint 变体 | 🔧 自定义 Extractor |
| 4 | **Authentication** | 自定义 `FromRequestParts` Extractor，从 `Authorization: Bearer` 头提取 key，查 CompiledSnapshot 的 HashMap，验证 key 有效并解析 tenant/key 元数据 | 🔧 自定义 Extractor（模式清晰） |
| 5 | **Authorization** | 在 Handler 中读取 key 元数据与 CompiledSnapshot 里的 policy 规则，检查该 key 是否有权访问请求的 model/操作，纯内存匹配 | 🔧 自定义逻辑（无外部依赖） |
| 6 | **RateLimit** | 两层检查：① local shadow（内存 governor/GCRA 计数器，快速拒绝明显超限，保护 Redis）→ ② Redis 原子操作检查剩余配额（> 0 则放行）；实际 input/output token 消耗在响应后扣除 | 🔧 需配合 Redis |
| 7 | **PreCall Guardrail** | `reqwest` 调用外部 HTTP guardrail 服务，await 结果，失败则 early return | 🔧 标准 HTTP 客户端调用 |
| 8 | **Cache Lookup** | `redis` crate GET，命中则直接构造响应返回，跳过后续阶段 | 🔧 自定义（逻辑简单） |
| 9 | **RouteSelect** | 读取 CompiledSnapshot 的 upstream 列表，按策略（round-robin / weighted / failover）选择，纯内存计算 | 🔧 自定义调度逻辑 |
| 10 | **PreUpstream** | 可变克隆请求体，注入 `system` message，替换模板变量，覆盖参数 | 🔧 自定义变换逻辑 |
| 11 | **UpstreamHeaders** | 读取选定 provider 的 credential，拼装 `Authorization`、`x-api-key`、`api-version` 等头 | 🔧 自定义（per-provider 分支） |
| 12 | **StreamChunk** | 按 `TransportMode` 分发三条路径：`Json`（完整响应代理）、`SseStream`（`EventStream` → `StreamEvent` 解析 + OpenAI SSE 渲染）、`BinaryStream`（`ByteStream` 原始字节透传）；其中 `SseStream` 路径含帧边界解析、多 Provider 格式转码、首字节前 fallback | ⚠️ 技术挑战最高（SseStream 路径） |
| 13 | **PostCall** | tokio `spawn` 后台任务：写 Redis 用量、更新计费、写语义缓存、调 webhook | 🔧 后台任务（需注意不阻塞响应） |
| 14 | **OnError** | 匹配自定义 `GatewayError` 枚举，转为 `axum::Json` 标准错误响应；可选触发 fallback 重试 | 🔧 自定义错误类型 + `IntoResponse` |

**图例**：
- ✅ **社区 crate**：几行配置，不需要自己写逻辑
- 🔧 **自定义但模式清晰**：需要写代码，但思路直接，没有坑
- ⚠️ **真正有挑战**：需要深入理解底层机制，容易出 bug

---

### 4.4 三种传输模式

由 `CanonicalRequest::transport_mode()` 决定，`stream_chunk` 按 `TransportMode` 分发到三条路径。

**TransportMode::Json（完整 JSON 响应）**
- `ProviderOutput::Json`：读取完整上游响应体
- 解析为 canonical response，应用 post-call guardrails
- 提取 usage/cost，可选缓存，返回 JSON
- 适用端点：Embeddings、Images、非流式 Chat/Responses

**TransportMode::SseStream（SSE 事件流）**
- `ProviderOutput::EventStream`：codec 将上游格式（OpenAI SSE / Anthropic event/data / Gemini JSON lines / Bedrock binary eventstream）统一解析为 `StreamEvent`
- **首字节前**可以重试/fallback；**首字节后**不再重试，该 provider 调用确定
- 逐事件渲染为 OpenAI SSE 输出给客户端
- 遇到 `StreamEvent::Usage` 时增量累积 token 计数，写入 `ctx.usage`
- 流结束时异步结算
- 适用端点：流式 Chat/Responses（`stream: true`）

**TransportMode::BinaryStream（原始字节流）**
- `ProviderOutput::ByteStream`：codec 不解析内容，直接透传上游字节流
- `stream_chunk` 将字节直接转发给客户端，不经过 `StreamEvent` 解析
- usage 在请求前已知（按字符数计算），流内不累积
- 适用端点：AudioSpeech（返回 MP3/Opus/AAC 等音频数据）

**核心原则：统一解析转发（SseStream 路径）**
- 所有流式上游响应（无论 OpenAI 兼容格式还是 Anthropic/Gemini/Bedrock 格式）统一解析为内部 `StreamEvent`
- 再渲染为 OpenAI SSE 输出给客户端
- 这使 token 计数、guardrail、日志等后处理逻辑统一，无需为每种上游格式维护独立的转发路径

---

### 4.5 难度边界与实现要点

#### ✅ 直接用社区 crate，挂上去就完成

```rust
let app = Router::new()
    .route("/v1/chat/completions", post(chat_completions))
    .layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)), // 10MB
    )
    .with_state(state);
```

#### 🔧 需要自定义代码，但有清晰实现模式

**Authentication Extractor（阶段 4）**——实现 `axum::extract::FromRequestParts`，在路由匹配后、Handler 执行前自动运行。模式固定，社区有大量案例：

```rust
pub struct AuthenticatedKey(pub KeyMeta);

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedKey
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = GatewayError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let snapshot = app_state.snapshot.load();
        let token = extract_bearer_token(&parts.headers)?;
        // snapshot.keys: HashMap<String, KeyMeta>，以明文 Bearer token 为键
        // etcd 中 apikey 记录的 "key" 字段（明文）在编译快照时作为 HashMap 的 key
        let key_meta = snapshot.keys.get(token).ok_or(GatewayError::Unauthorized)?;
        Ok(AuthenticatedKey(key_meta.clone()))
    }
}
```

#### ⚠️ 技术上真正有挑战的部分

**流式代理（阶段 12）**——这是整个 Gateway 最复杂的地方，有三个独立难点：

**难点 A：流式帧边界处理**

SSE 帧边界不一定与 TCP 包边界对齐。一个 SSE 事件可能跨多个 TCP 包到达，也可能一个 TCP 包包含多个 SSE 事件。解析器需要维护跨帧状态机：

```
TCP packet arrives → append to buffer → find complete frame delimiter (\n\n)
  → extract complete frame → parse into StreamEvent → render as OpenAI SSE
  → leave remaining bytes in buffer, wait for next packet
```

挑战：状态机必须是零 panic——流式响应一旦开始发送，panic 会让客户端收到截断的响应。同时需要处理各 Provider 的格式差异（OpenAI SSE、Anthropic event/data、Gemini JSON lines、Bedrock 二进制 eventstream）。

**难点 B：多 Provider 格式转码**

各 Provider 流格式各异，需要分别实现解析器并统一输出为 `StreamEvent`：

```
upstream Anthropic SSE frame
  data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}
         ↓ parse into StreamEvent
StreamEvent::Delta("Hello")
         ↓ render as OpenAI SSE
  data: {"id":"...","choices":[{"delta":{"content":"Hello"}}]}
```

挑战：Bedrock 使用 `application/vnd.amazon.eventstream` 二进制帧格式（非 SSE），需要独立的二进制解析器；Vertex AI 使用 JSON lines。每种格式的帧结束信号和 usage 字段位置不同，需要全部统一到 `StreamEvent::Usage`。

**难点 C：首字节前 fallback**

在收到上游第一个响应字节之前，如果发生错误（连接超时、上游 5xx），可以透明切换到备用 provider。但一旦第一个字节已经发给客户端，就无法再 fallback（HTTP 响应头已发出，状态码已定）：

```
send request ──▶ upstream A
               │
               ├── success: start streaming to client ──▶ (no fallback after this point)
               │
               └── failure (before first byte):
                     ↓
                   switch to upstream B, retry request ──▶ client
```

挑战：需要精确区分"响应头/首字节已发"和"尚未发出任何字节"两个状态，并在 fallback 时重置内部状态（重新执行 RouteSelect）。这个状态判断在 async 流式代码中容易出 race condition。

**难点 D：客户端断开时立即 cancel 上游请求**

客户端中途断开连接（例如用户刷新浏览器），axum 的 SSE response body future 会被 drop/cancel。此时必须同时 cancel 上游 HTTP 请求，否则 gateway 会继续消耗上游 token 和并发槽位：

```
client disconnects
  ↓ axum drops the SSE body future
  ↓ tokio cancels the stream_chunk task
  ↓ ConcurrencyGuard::drop() triggers → ZREM (并发槽释放)
  ↓ hyper upstream request also dropped → TCP connection returned to pool or closed
```

挑战：整条流式 pipeline 必须是 cancel-safe——每个 `await` 点都可能被取消，中间状态不能泄漏（Redis ZADD 后 ZREM 不执行、上游连接不归还连接池）。实现方式：所有资源持有用 RAII guard 包装，而不是在 `finally` 块中手动清理。

---

### 4.6 Pipeline 执行模型

> **设计原则**：所有请求走相同的固定阶段序列。每个阶段内部查询配置快照，有配置则执行实际操作，无配置则 no-op。不需要外部 bool 开关，不需要动态组合 stage 列表。

#### RequestContext：贯穿全程的请求上下文

每个阶段统一读写同一个上下文，随着 pipeline 推进逐步填充：

```rust
struct RequestContext {
    // ── Decode 后填入 ──────────────────────────────
    request: CanonicalRequest,          // Chat / Embeddings / ... 统一枚举，覆盖所有端点

    // ── Authentication 后填入 ───────────────────────
    key_meta: KeyMeta,           // 身份标识，各阶段按 id 查配置快照

    // ── RouteSelect 后填入 ──────────────────────────
    selected_upstream: Upstream,

    // ── StreamChunk 后填入 ──────────────────────────
    usage: Option<TokenUsage>,           // 用于 PostCall 计费
    response_cached: bool,               // 是否命中缓存
}
```

#### 固定阶段序列

所有请求都走以下固定顺序。每个阶段内部按 `key_meta` id 查快照，无配置则直接返回 `Ok(())` 继续下一阶段：

| 阶段 | 说明 |
|------|------|
| Authentication | axum Extractor，解析 Bearer token，加载 KeyMeta |
| Authorization | 检查 key 是否有权访问目标模型/路由 |
| RateLimit | 按 key/team/user 查限速配置；有配置则查 Redis，无配置则 no-op |
| PreCall Guardrail | 查 guardrail 规则；有配置则调外部 HTTP callback，无配置则 no-op |
| Cache Lookup | 查缓存策略；有配置则查 Redis/语义缓存，命中则短路返回，无配置则 no-op |
| PreUpstream | 查 prompt template 配置；有则渲染注入，无则 no-op |
| RouteSelect | 按路由策略选上游 provider + model |
| UpstreamHeaders | 拼装上游鉴权头（API key 等） |
| StreamChunk | 转发请求，流式/非流式代理核心逻辑 |
| PostCall | 异步：扣费、写日志，不阻塞响应 |

#### Handler 入口

直接顺序调用，无抽象层：

```rust
async fn chat_completions(
    State(state): State<AppState>,
    AuthenticatedKey(key_meta): AuthenticatedKey,  // Authentication（Extractor）
    Json(body): Json<ChatCompletionRequest>,        // Decode（axum 自动完成）
) -> Result<Response, GatewayError> {
    let mut ctx = RequestContext::new(CanonicalRequest::Chat(body), key_meta);

    authorization::check(&ctx, &state)?;
    rate_limit::check(&ctx, &state).await?;      // 有配置则查 Redis，否则 no-op
    guardrail::pre_call(&ctx, &state).await?;    // 有配置则调 HTTP callback，否则 no-op
    if let Some(resp) = cache::lookup(&ctx, &state).await? {
        let post_call_ctx = ctx.into_post_call_context();
        tokio::spawn(post_call::run(post_call_ctx, state)); // 异步计费/日志
        return Ok(resp);
    }
    pre_upstream::apply(&mut ctx, &state)?;      // 有配置则渲染 prompt template，否则 no-op
    route_select::resolve(&mut ctx, &state)?;
    upstream_headers::inject(&mut ctx, &state)?;

    // stream_chunk::proxy 阻塞直到响应（含流）全部写入客户端连接，ctx.usage 已填充
    let resp = stream_chunk::proxy(&mut ctx, &state).await?;

    let post_call_ctx = ctx.into_post_call_context();
    tokio::spawn(post_call::run(post_call_ctx, state));     // 异步：扣费 + 写日志，不阻塞响应
    Ok(resp)
}
```

#### 多端点 Pipeline 复用模型

`Operation` 枚举定义了 8 种 API 类型，按 pipeline 复用度可分为 4 组：

| 组别 | API | 核心特征 | Pipeline 复用度 |
|------|-----|---------|----------------|
| **A. 流式 JSON (SSE)** | `ChatCompletions`, `Responses` | 有流式/非流式两条路径，需要 SSE 编解码、首字节前 fallback | ~90% |
| **B. 纯 JSON 请求-响应** | `Embeddings`, `Images` | 永远非流式，不需要 SSE 转码 | ~70% |
| **C. 二进制/多媒体** | `AudioTranscription`, `AudioSpeech` | 请求可能是 multipart 或响应是二进制流（非 SSE） | ~55% |
| **D. 长连接/特殊协议** | `RealtimeSession`, `McpCall` | WebSocket 或自定义协议，脱离 HTTP request-response 模型 | ~20-30% |

**核心结论**：A 组和 B 组可以共享同一个 `run_pipeline()` 通用函数。

##### 各阶段复用明细

以 `chat_completions` 为基准，逐阶段对比其余 API 的复用情况：

| Pipeline 阶段 | ChatCompletions | Responses | Embeddings | Images | AudioTranscription | AudioSpeech | RealtimeSession | McpCall |
|---|---|---|---|---|---|---|---|---|
| Decode | JSON | JSON | JSON | JSON/Multipart | **Multipart** | JSON | **WebSocket** | HTTP |
| Authentication | ✅ Extractor | ✅ 同左 | ✅ 同左 | ✅ 同左 | ✅ 同左 | ✅ 同左 | ⚠️ WS 握手 | ⚠️ 不同 |
| Authorization | ✅ | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 |
| RateLimit | ✅ | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 并发租约 | ✅ 同 |
| PreCall Guardrail | ✅ | ✅ 同 | ✅ no-op | ⚠️ 可选 | ❌ 不适用 | ❌ 不适用 | ❌ 不适用 | ❌ 不适用 |
| Cache Lookup | ✅ | ✅ 同 | ⚠️ 可选 | ⚠️ 可选 | ❌ | ❌ | ❌ | ❌ |
| PreUpstream | ✅ | ✅ 同 | ❌ no-op | ❌ no-op | ❌ no-op | ❌ no-op | ❌ | ❌ |
| RouteSelect | ✅ | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 |
| UpstreamHeaders | ✅ | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 |
| StreamChunk | ✅ SSE+非流 | ✅ SSE+非流 | ✅ **仅非流** | ✅ **仅非流** | ✅ **仅非流** | ⚠️ **二进制流** | ❌ 不适用 | ⚠️ 独立 |
| PostCall | ✅ | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ 同 | ✅ | ✅ 同 |

图例：✅ = 完全复用 ｜ ⚠️ = 部分复用/需适配 ｜ ❌ = 不适用

##### 通用 `run_pipeline` 函数

A 组（Chat + Responses）和 B 组（Embeddings + Images）共享同一个 pipeline 执行函数，差异通过 `CanonicalRequest` 枚举和 `ProviderCodec` trait 的多态消化：

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
        let post_call_ctx = ctx.into_post_call_context();
        tokio::spawn(post_call::run(post_call_ctx, state));
        return Ok(resp);
    }
    pre_upstream::apply(&mut ctx, &state)?;
    route_select::resolve(&mut ctx, &state)?;
    upstream_headers::inject(&mut ctx, &state)?;

    // stream_chunk::proxy 语义（方案 A — 阻塞直到传输完成）：
    //   - TransportMode::Json / BinaryStream：完整响应体发送完毕后返回，ctx.usage 已填充。
    //   - TransportMode::SseStream：将全部 SSE 帧（含 data:[DONE]）写入客户端连接后返回，
    //     ctx.usage 在流结束时由 StreamEvent::Usage 累积写入，proxy 返回时已可安全读取。
    let resp = stream_chunk::proxy(&mut ctx, &state).await?;

    // 从 RequestContext 中提取 post_call 所需的最小数据集（PostCallContext），
    // 确保 SSE stream 响应体的生命周期不与 post_call task 共享引用。
    // PostCallContext 包含：usage、key_id、model、provider_id、cache_hit 等，
    // 均为 owned 值（String、u64、bool），不借用 ctx 内部数据。
    let post_call_ctx = ctx.into_post_call_context();
    tokio::spawn(post_call::run(post_call_ctx, state));   // 异步：扣费 + 写日志，不阻塞调用方
    Ok(resp)
}
```

各 handler 只需负责 Decode + 调用 `run_pipeline`：

```rust
async fn chat_completions(
    State(state): State<AppState>,
    AuthenticatedKey(key_meta): AuthenticatedKey,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Response, GatewayError> {
    run_pipeline(state, key_meta, CanonicalRequest::Chat(body)).await
}

async fn embeddings(
    State(state): State<AppState>,
    AuthenticatedKey(key_meta): AuthenticatedKey,
    Json(body): Json<EmbeddingsRequest>,
) -> Result<Response, GatewayError> {
    run_pipeline(state, key_meta, CanonicalRequest::Embeddings(body)).await
}
```

##### StreamChunk 内部分发

`stream_chunk::proxy` **只关心传输方式，不感知 Operation**。由 `CanonicalRequest::transport_mode()` 决定走哪条转发路径——这是 `CanonicalRequest` 的内置固定能力，与 Provider 无关：

```rust
// stream_chunk::proxy — 按 TransportMode 分发，不出现任何 Operation 分支
pub async fn proxy(ctx: &mut RequestContext, state: &AppState) -> Result<Response, GatewayError> {
    match ctx.request.transport_mode() {
        TransportMode::SseStream    => proxy_sse_stream(ctx, state).await,
        TransportMode::Json         => proxy_json_response(ctx, state).await,
        TransportMode::BinaryStream => proxy_binary_stream(ctx, state).await,
    }
}
```

**职责边界**：

- `CanonicalRequest::transport_mode()`（aisix-types）：回答"客户端期待以什么方式接收响应"，由 Operation + `stream` 字段决定，Provider 无关。新增端点时只需在此方法中添加一行。
- `ProviderCodec`（aisix-providers）：各 Provider 独立实现，负责格式转换（build_request / parse_response），不感知 transport_mode。
- `stream_chunk`（pipeline）：只依赖 `TransportMode` 枚举做转发分发，三个变体各自独立实现，新增传输方式时才需改动。

##### C/D 组的处理策略

**C 组（Audio）**：前半段 pipeline（Auth → RateLimit → RouteSelect）通过 `run_pipeline` 复用，仅 Decode 阶段（multipart 解析）和 StreamChunk（二进制流）需要独立实现。

**D 组（Realtime + MCP）**：本质上是不同的协议，建议独立的 handler 代码路径，仅复用基础模块（Authentication Extractor、RouteSelect、RateLimit 的概念和实现），不走 `run_pipeline`。

#### 代码组织建议

每个 stage 独立文件，不互相依赖：

```
src/pipeline/
├── context.rs          // RequestContext definition
├── authorization.rs
├── guardrail.rs
├── rate_limit.rs
├── cache.rs
├── pre_upstream.rs
├── route_select.rs
├── upstream_headers.rs
├── stream_chunk.rs
└── post_call.rs
```

---

### 4.7 State 注入与热加载

所有 Handler 阶段通过 axum 的 `State` 机制共享配置，无需任何全局变量或 Mutex：

```rust
// AppState 定义 —— 克隆成本极低（都是 Arc）
#[derive(Clone)]
pub struct AppState {
    /// 当前生效的编译快照，ArcSwap 允许无锁原子替换
    /// （ArcSwap 本身内部持有 Arc，无需外层再包 Arc）
    pub snapshot: ArcSwap<CompiledSnapshot>,
    /// Redis 连接池（bb8 或 deadpool）
    pub redis: RedisPool,
    /// 上游 HTTP 客户端（复用连接池）
    pub upstream_client: reqwest::Client,
}

// Handler 签名示例
async fn chat_completions(
    State(state): State<AppState>,
    AuthenticatedKey(key_meta): AuthenticatedKey,  // 自定义 Extractor
    Json(req): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse, GatewayError> {
    // 读取快照 —— load() 是原子操作，无锁
    let snapshot = state.snapshot.load();
    // snapshot 是 Arc<CompiledSnapshot>，在本次请求全程持有
    // 即使控制面推来新配置，当前请求用的快照不受影响
    ...
}
```

**热加载流程**：

```
etcd watch triggered
      │
      ▼
background task (tokio::spawn)
  recompile config → Arc<CompiledSnapshot>
      │
      ▼
state.snapshot.store(new_snapshot)  ← atomic swap, < 1μs
      │
      ▼
new requests use new snapshot automatically
in-flight requests keep old snapshot until done (Arc refcount)
```

**为什么不用 `RwLock<CompiledSnapshot>`**：
- RwLock 在高并发下有读者饥饿风险
- ArcSwap 的 `load()` 是无锁操作，只有 `store()` 需要短暂原子交换
- 配置更新频率远低于请求频率，ArcSwap 完全匹配这个场景

---

### 4.8 实现优先级建议

基于上述分析，建议按以下顺序攻克技术风险：

```
Step 1 (baseline validation)
  └── run a non-streaming chat completion end-to-end
      validate: Authentication → RateLimit → RouteSelect → sync HTTP proxy → PostCall
      all 🔧 level, no ⚠️, good for building confidence first

Step 2 (streaming core)
  └── tackle the three StreamChunk hard points
       order: frame boundary parsing → multi-provider transcoding → pre-first-byte fallback
      highest technical difficulty; test against real providers early

Step 3 (full features)
  └── incrementally add: Guardrail → semantic cache → full Policy rules
      all 🔧 level, add as needed once the core path is proven
```

> **注意**：上述 Step 1/2/3 是技术风险攻克顺序，与第十一章的 MVP Phase 1/2/3 是两个不同维度：前者描述"先做哪个技术点"，后者描述"按季度交付的产品里程碑"。

---

## 五、状态管理与存储

### 四层存储模型

| 层级 | 存储位置 | 内容 | 访问模式 |
|------|---------|------|---------|
| **L1 热路径** | 进程内 `Arc<CompiledSnapshot>` | 路由索引、策略表、模板、正则、Provider 注册表 | 无锁读，ArcSwap 原子切换 |
| **L2 分布式计数** | Redis | RPM/TPM 计数器、并发租约、冷却标记、实时花费 | Lua 脚本原子操作 |
| **L3 共享缓存** | Redis / S3 / GCS | 响应缓存（非流式）、语义缓存向量 | 异步读写 |
| **L4 持久真相** | PostgreSQL | 使用量账本、审计日志、预算定义、定价表 | **控制面持有，数据面不直接访问**；数据面通过结构化日志输出 UsageEvent，由外部 log agent（Vector/Fluentd 等）采集后写入 |

### 限流器两层模型

```
request arrives
  │
  ▼
[local shadow rate limiter] ─── in-memory governor (GCRA) counter, ultra-low cost
  │                                obvious overages → reject (protect Redis)
  │
  ▼ pass
[Redis authoritative rate limiter] ─── Lua atomic check + reserve
  │
  ├── reject → return 429
  │
  ▼ pass
[execute request]
  │
  ▼
[async settle] ─── actual usage delta update Redis + emit UsageEvent to structlog
```

### 限流维度

| 维度 | 参数 | Redis Key 模式 |
|------|------|----------------|
| TPM | `tpm_limit` | `rl:tpm:{scope}:{id}:{model}:{window}` |
| RPM | `rpm_limit` | `rl:rpm:{scope}:{id}:{model}:{window}` |
| 并发 | `max_parallel_requests` | `rl:cc:{scope}:{id}:{model}` |
| 冷却 | `cooldown_time` | `cooldown:{target_id}` |

### TPM 处理流程

```
request start:
  check remaining quota > 0 → allow

request complete:
  deduct actual input + output tokens from Redis quota
```

### 并发租约（Redis Sorted Set）

```
request start:
  ZADD rl:cc:{scope}:{id} {expires_at} {request_id}
  ZREMRANGEBYSCORE rl:cc:{scope}:{id} 0 {now}   ← evict expired entries
  ZCARD rl:cc:{scope}:{id} > limit → reject

request complete (正常结束 / 客户端断开 / 超时):
  ZREM rl:cc:{scope}:{id} {request_id}
```

**客户端断开时的租约清理保证**：

- 并发计数器通过 RAII guard（`ConcurrencyGuard`）持有，实现 `Drop` trait：
  ```rust
  impl Drop for ConcurrencyGuard {
      fn drop(&mut self) {
          // 无论正常结束、panic、客户端断开，均触发 ZREM。
          // Drop::drop 是同步函数，不能直接调用 async Redis 操作，
          // 因此 spawn 一个独立 task 来执行异步 ZREM。
          let key = self.key.clone();
          let request_id = self.request_id.clone();
          let redis = self.redis.clone();
          tokio::spawn(async move {
              let _ = redis.zrem(&key, &request_id).await;
          });
      }
  }
  ```
- `stream_chunk` future 被 cancel 时（客户端断开），tokio 会 drop 持有 guard 的 future，
  触发 `Drop::drop`，进而 spawn 异步 ZREM task（cancel-safe）。
- 同时设置 `expires_at = now + max_request_timeout`，作为双重保险：
  即使 gateway 崩溃未执行 Drop，过期条目也会在下次请求时被 `ZREMRANGEBYSCORE` 自动清理。

### Spend 追踪：异步 Spawn

**关键：请求路径永不阻塞在日志/计费写入上。**

`post_call::run` 通过 `tokio::spawn` 在独立 task 中执行，不阻塞响应返回：

```
request path (run_pipeline)
  → stream_chunk::proxy 返回 Response
  → tokio::spawn(post_call::run(post_call_ctx, state))
  → 立即返回 Response 给客户端

post_call task (独立 tokio task)
  ├── increment Redis real-time spend counter
  ├── deduct actual token usage from Redis quota
  └── write structured log (JSON, one line per event)
```

`post_call::run` 接收 `PostCallContext`（从 `RequestContext` 拆分出的最小数据集），
确保 SSE stream 响应体的生命周期不与 post_call task 共享引用。

### 预算执行层级

```
Global Proxy Budget
  └─ Virtual Key Budget (+ per-model budget)
       └─ Customer (end-user) Budget
```

每个层级独立检查，任一层级拒绝即返回 429。

> **Team / Team Member 层级**：当前暂不引入（见第十五章 TODO）。待积累足够生产用户迭代后再评估是否需要。

---

## 六、Provider 适配器设计

### 三层架构

```
┌────────────────────────────────────┐
│  Canonical API Layer               │  unified request/response types
│  (aisix-types)                     │  CanonicalRequest / CanonicalResponse
├────────────────────────────────────┤
│  Provider Codec Layer              │  one codec per provider
│  (aisix-providers)                 │  - OpenAICompatCodec (generic)
│                                    │    ↳ reused for OpenAI / Azure OpenAI /
│                                    │      Ollama / vLLM / Groq etc.
│                                    │  - AnthropicCodec
│                                    │  - VertexCodec (incl. Google OAuth 2.0)
│                                    │  - BedrockCodec (incl. binary eventstream)
├────────────────────────────────────┤
│  Shared Transport Layer            │  unified upstream HTTP client
│  (reqwest client pool)             │  - connection pool (by scheme+host+port)
│                                    │  - HTTP/2 preferred, HTTP/1.1 keepalive
│                                    │  - DNS cache
│                                    │  - per-origin timeout config
└────────────────────────────────────┘
```

### OpenAICompatCodec

大多数 Provider 使用与 OpenAI 相同的 REST 接口格式。`OpenAICompatCodec` 是一个通用实现，注册 Provider 时只需传入不同的 base URL 和 auth 策略即可复用，无需为每个兼容 Provider 单独写 codec：

```rust
// 注册示例
registry.register("openai-us",     OpenAICompatCodec::new("https://api.openai.com",      BearerAuth(openai_key)));
registry.register("azure-eastus",  OpenAICompatCodec::new("https://myazure.openai.azure.com", AzureApiKeyAuth(azure_key)));
registry.register("ollama-local",  OpenAICompatCodec::new("http://localhost:11434",       NoAuth));
registry.register("groq",          OpenAICompatCodec::new("https://api.groq.com/openai", BearerAuth(groq_key)));
```

非兼容 Provider 各自独立实现 `ProviderCodec`：
- **AnthropicCodec**：`x-api-key` 头，Anthropic 专有消息格式，`event:` + `data:` 双行 SSE
- **VertexCodec**：Google OAuth 2.0 service account JWT，Gemini JSON lines 流格式
- **BedrockCodec**：AWS SigV4 签名，`application/vnd.amazon.eventstream` 二进制帧（工作量最大）

### Provider 能力声明

每个 Provider 声明其支持的能力，供路由和策略系统参考：

```rust
pub struct ProviderCapabilities {
    pub chat: bool,
    pub responses: bool,
    pub embeddings: bool,
    pub images: bool,
    pub audio_transcription: bool,
    pub audio_speech: bool,
    pub realtime: bool,
    pub streaming: bool,
    pub tool_calling: bool,
    pub vision: bool,
    pub token_accounting_fidelity: TokenFidelity,  // Exact/Estimated/None
}
```

### 请求转换流水线

```
CanonicalRequest
  → apply model-specific prompt template
  → drop/modify params (per policy)
  → map logical model → provider deployment
  → ProviderCodec builds HTTP request
  → send to upstream
```

### 流式适配

```
upstream provider SSE formats vary:
  OpenAI    → data: {"choices":[{"delta":{"content":"Hi"}}]}
  Anthropic → event: content_block_delta / data: {"delta":{"text":"Hi"}}
  Gemini    → JSON lines with candidates

         ↓ unified conversion to internal stream ↓

StreamEvent::Delta(Bytes)      // content chunk
StreamEvent::Usage(UsageDelta) // incremental tokens
StreamEvent::Done              // end

         ↓ render as OpenAI-compatible SSE ↓

data: {"choices":[{"delta":{"content":"Hi"}}]}
```

### 错误归一化

```rust
pub enum ErrorKind {
    Auth,                    // 401
    Permission,              // 403
    BadRequest,              // 400
    RateLimited,             // 429
    ContextWindowExceeded,   // 400 (provider-specific)
    ContentFiltered,         // 400 (provider-specific)
    Timeout,                 // 504
    UpstreamUnavailable,     // 502
    Overloaded,              // 529
    Internal,                // 500
    Unsupported,             // 501
}

impl ErrorKind {
    /// 基于错误类型判断是否可重试，统一重试/fallback 决策。
    /// 重试和 fallback 逻辑基于此方法返回值，不基于 provider 特定字符串。
    pub fn retryable(&self) -> bool {
        matches!(self, RateLimited | Timeout | UpstreamUnavailable | Overloaded)
    }
}

// 每个归一化错误携带：
// - provider_status: Option<u16>
// - provider_code: Option<String>
// - provider_request_id: Option<String>
```

---

## 七、配置系统

### YAML Schema 示例

```yaml
# aisix-gateway.yaml — 进程启动配置
# 仅包含启动时静态所需内容。
# Provider、Model、Virtual Key、Limits、Guardrail 等运行时配置
# 均由 aisix-admin 写入 etcd，aisix-gateway 启动后通过 watch 动态加载。

server:
  listen: "0.0.0.0:4000"
  request_body_limit_mb: 8
  response_body_limit_mb: 64

etcd:
  endpoints:
    - "http://etcd:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000

log:
  level: "info"          # trace / debug / info / warn / error

runtime:
  worker_threads: 4      # tokio worker 线程数，0 = CPU 核心数
  max_blocking_threads: 64
```

运行时配置（Provider、Model、API Key）的数据结构见下方 [etcd 数据模型](#etcd-数据模型)。

### etcd 数据模型

```yaml
# etcd key 前缀设计
/aisix/
  ├── providers/
  │   └── {provider_id}    # Provider 绑定信息
  │
  ├── models/
  │   └── {model_id}       # 大模型定义（含限流）
  │
  ├── apikeys/
  │   └── {apikey_id}      # 客户端调用身份（含限流）
  │
  └── policies/
      └── {policy_id}      # 可复用限流策略（可被 provider/model/apikey 引用）
```

**policy**：

```json
{
  "id": "standard-tier",
  "rate_limit": {
    "rpm": 500,
    "rpd": 5000,
    "tpm": 100000,
    "tpd": 1000000,
    "concurrency": 10
  }
}
```

**provider**：

```json
{
  "id": "openai-us",
  "kind": "openai",
  "base_url": "https://api.openai.com",
  "auth": { "secret_ref": "env:OPENAI_API_KEY" },
  "policy_id": "standard-tier"
}
```

**model**：

```json
{
  "id": "gpt-4o-mini",
  "provider_id": "openai-us",
  "upstream_model": "gpt-4.1-mini",
  "policy_id": "standard-tier"
}
```

**apikey**：

> API Key 以**明文**存储在 etcd 中。编译快照时，以 `key` 字段的明文值为键建立 `HashMap<String, KeyMeta>`，Authentication 阶段直接比较 Bearer token 与明文值。安全边界由 etcd 访问权限和 TLS 传输保障。

```json
{
  "id": "key-abc123",
  "key": "my-secret-key",
  "allowed_models": ["gpt-4o-mini", "claude-sonnet"],
  "policy_id": "standard-tier",
  "rate_limit": {       // 内联限流覆盖 policy 定义（优先级更高）
    "rpm": 100,
    "rpd": 1000,
    "tpm": 50000,
    "tpd": 500000,
    "concurrency": 5
  }
}
```

### 三层限流语义

`rate_limit` 字段内联在 provider、model、apikey 三个资源中，各层独立检查，**任一层超限即返回 429**：

| 层级 | 含义 | 典型用途 |
|------|------|---------|
| **provider** | 该 provider 全局上限（对应 provider 账号的配额） | 防止网关整体打爆上游账号限额 |
| **model** | 该 model 的全局上限（所有 apikey 汇总） | 控制某个模型的整体用量 |
| **apikey** | 该 key 的调用上限 | 对接入方/租户做隔离 |

三层各自独立计数，执行顺序：provider → model → apikey，遇到任意一层超限立即返回 429，不再继续检查后续层。

**policy_id 与内联 rate_limit 的优先级**：若资源同时设置了 `policy_id` 和内联 `rate_limit`，以内联 `rate_limit` 为准（具体覆盖通用，inline 覆盖 policy_id）；未设置内联 `rate_limit` 时，使用 `policy_id` 指向的 policy。两者均未设置则该层不做限流。

### 配置编译流程

```
aisix-gateway starts
  │
  ▼
connect to etcd cluster
  │
  ▼
[1] GET /aisix/ prefix (range) → fetch full config (response carries current revision N)
  │
  ▼
[2] resolve secret references (env/KMS/Vault → actual values)
  │
  ▼
[3] three-layer validation
  │    ├── schema validation: required fields, enum values, type correctness
  │    ├── semantic validation: referential integrity, no-cycle fallback, sane limits
  │    └── runtime pre-check: URL format, auth completeness, capability compatibility
  │
  ▼
[4] compile into CompiledSnapshot
  │    ├── build route index (alias → group, wildcard matcher)
  │    ├── build policy lookup table (key → EffectivePolicy)
  │    ├── pre-compile regex/templates
  │    ├── build Provider registry
  │    └── build guardrail chain
  │
  ▼
[5] ArcSwap atomic swap → start serving traffic
  │
  ▼
[6] WATCH /aisix/ prefix (from revision N+1) → continuous change listening
  │
  ├── on PUT event
  │    ├── debounce (250-500ms merge window)
  │    ├── merge changes into local config
  │    ├── re-validate + recompile
  │    └── ArcSwap atomic swap (keep old snapshot if validation fails)
```

### 热加载安全机制

| 机制 | 说明 |
|------|------|
| **去抖窗口** | 250-500ms，合并短时间内的多次变更 |
| **最小重建间隔** | 避免频繁切换导致 CPU 尖峰 |
| **后台编译** | 编译任务在独立 tokio task 上执行，不阻塞请求路径 |
| **原子指针切换** | `ArcSwap<CompiledSnapshot>`，无锁读取 |
| **失败保护** | 验证失败则保留旧快照，拒绝新配置 |

### 验证错误报告示例

```
apikeys[0].allowed_models[1]:
  unknown model "gpt-4o-mini-fast"
  did you mean "gpt-4o-mini"?
```

---

## 八、Cargo Workspace 结构

### 目录布局

```
aisix/
├── Cargo.toml                    # workspace root
├── bin/                          # ── entry (L4) ──
│   └── aisix-gateway/                   # binary entry point
│       └── main.rs
└── crates/
    │
    │  ── foundation (L0) ──
    ├── aisix-types/              # shared types: CanonicalRequest/Response, Usage, IDs, Error
    │
    │  ── core infra (L1) ──
    ├── aisix-core/               # core abstractions: RequestContext, GatewayState, pipeline orchestration
    ├── aisix-config/             # etcd watch + compiled snapshot + validation + hot-reload
    ├── aisix-storage/            # Redis (counters/cache) + secret resolution (PG owned by control plane)
    │
    │  ── domain modules (L2) ──
    ├── aisix-auth/               # Virtual Key 认证（Bearer token 查快照）
    ├── aisix-policy/             # hierarchical policy resolution, model/label access control, param mutation
    ├── aisix-router/             # model resolution, fallback, cooldown
    ├── aisix-ratelimit/          # RPM/TPM/concurrency/budget checks, local shadow + Redis authoritative
    ├── aisix-cache/              # memory/Redis/Disk/S3/semantic cache backends
    ├── aisix-providers/          # provider codecs, request/response transcoding, error normalization
    ├── aisix-guardrail/          # HTTP callback engine, pre/post-call guardrail
    ├── aisix-spend/              # pricing, cost calculation, async batch billing, budget reconciliation
    ├── aisix-observability/      # tracing, Prometheus, OTEL, callback sink fanout
    │
    │  ── orchestration (L3) ──
    ├── aisix-runtime/            # runtime composition, background tasks, snapshot lifecycle, registry
    └── aisix-server/             # axum routing, request extraction, SSE response, health/metrics endpoints
```

### Crate 职责表

| Crate | 职责 | 拥有的状态/数据 |
|-------|------|----------------|
| `aisix-types` | 共享类型定义 | CanonicalRequest/Response, TransportMode, Usage, IDs, Error 枚举 |
| `aisix-core` | 核心抽象 | RequestContext, GatewayState |
| `aisix-config` | 配置系统 | CompiledSnapshot, etcd watcher, 验证器 |
| `aisix-storage` | Redis 计数器与缓存；密钥解析 | Redis repo, 密钥解析器（PG 不在数据面依赖中） |
| `aisix-auth` | 认证 | Principal 解析, Key 校验 |
| `aisix-policy` | 策略 | EffectivePolicy 合并, 访问决策 |
| `aisix-router` | 路由 | RouteDecision, 策略实现, 健康状态 |
| `aisix-ratelimit` | 限流 | RateDecision, 本地计数器 |
| `aisix-cache` | 缓存 | CacheBackend 实现, key builder |
| `aisix-providers` | Provider | 编解码器（`ProviderCodec` trait）, `ProviderOutput`（含 `EventStream`/`ByteStream`）, `StreamEvent`, 流式适配器, 错误映射 |
| `aisix-guardrail` | 安全护栏 | HTTP callback 引擎, 超时控制 |
| `aisix-spend` | 费用 | UsageEvent, 定价表, 批量管道 |
| `aisix-observability` | 可观测 | Metrics, tracing spans, callback sink |
| `aisix-runtime` | 运行时 | 后台任务注册, 快照生命周期 |
| `aisix-server` | HTTP 服务 | axum 路由, 中间件, SSE 处理|

### 依赖关系图

> 矢量图见 `aisix-crate-deps.drawio`。箭头方向：A → B 表示 A 依赖 B（向下指向被依赖层）。

```
┌─────────────────────────────────────────────────────────────────────────┐
│ L4  Entry                                                               │
│                                                                         │
│   aisix-gateway → aisix-server → aisix-runtime                                          │
├─────────────────────────────────────────────────────────────────────────┤
│ L3  Orchestration                                                       │
│                                                                         │
│   aisix-runtime → auth · policy · router · ratelimit                    │
│                   cache · providers · guardrail · spend                 │
│                   observability                                         │
├─────────────────────────────────────────────────────────────────────────┤
│ L2  Domain Modules                                                      │
│                                                                         │
│   aisix-auth · aisix-policy · aisix-router · aisix-ratelimit            │
│   aisix-cache · aisix-providers · aisix-guardrail · aisix-spend         │
│   aisix-observability                                                   │
├─────────────────────────────────────────────────────────────────────────┤
│ L1  Core Infra                                                          │
│                                                                         │
│   aisix-config  ·  aisix-storage                                        │
│       ↓                 ↓                                               │
│       └─── aisix-core ──┘                                               │
├─────────────────────────────────────────────────────────────────────────┤
│ L0  Foundation                                                          │
│                                                                         │
│   aisix-types                                                           │
└─────────────────────────────────────────────────────────────────────────┘

```

---

## 九、Crate 选型

| 用途 | Crate | 说明 |
|------|-------|------|
| HTTP Server | `axum` + `tower` | Tower 中间件生态，流式友好 |
| HTTP Client | `reqwest` | 上游 HTTP 调用（底层 hyper），连接池，流式响应 |
| Async Runtime | `tokio` | 多线程运行时 |
| 序列化 | `serde` + `serde_json` + `serde_yaml` | 配置/请求解析 |
| etcd 客户端 | `etcd-client` | etcd v3 客户端, 支持 watch/lease/TXN |

| Redis | `redis` | 异步 + Lua 脚本 |
| JWT | `jsonwebtoken` | JWT 认证 |
| 限流 | `governor` | Token Bucket / GCRA (sliding window)，用于本地 shadow 限流器 |
| 指标 | `prometheus` | Prometheus exporter |
| 追踪 | `tracing` + `opentelemetry` | 结构化日志 + OTEL |
| 配置原子切换 | `arc-swap` | ArcSwap 无锁读取 |
| UUID | `uuid` | request_id |
| 验证 | `validator` | 配置字段验证 |
| 字节处理 | `bytes` | 零拷贝 Bytes, 热 body 路径 |

---

## 十、与 LiteLLM 对比

| 维度 | LiteLLM (Python) | AISIX (Rust) |
|------|-----------------|--------------|
| **并发模型** | 单线程 async (GIL) | 多线程 MPMC + async |
| **类型安全** | 运行时动态 | 编译时静态 |
| **配置** | 可变运行时状态 | 不可变编译快照 |
| **Provider 集成** | 每个 Provider 自带完整客户端 | 共享传输层 + 编解码器 |
| **Spend 追踪** | 同步数据库写入 | 异步批量管道 |
| **扩展方式** | Python 用户直接注入任意代码 | HTTP callback（外部 HTTP 服务调用） |
| **部署** | Python 环境 + 依赖 | 单静态二进制 (~10MB) |
| **上手门槛** | 低 | 中（Rust 学习曲线） |
| **性能** | 基准 1x | 预期 10-50x 吞吐，p99 远低 |

### AISIX 牺牲的
- Python 用户无法直接注入任意代码
- 新 Provider 集成需要写 Rust 编解码器
- 开发迭代速度不如 Python

### AISIX 获得的
- 极低网关开销（代理层 p99 < 1ms）
- 高并发下内存稳定（无 GC）
- 单二进制部署，运维简单
- 编译时类型安全，减少运行时意外
- 流式场景零拷贝，高并发低延迟
- 配置热加载零停机（etcd watch + ArcSwap）

---

## 十一、实施阶段

### Phase 1 — MVP（核心数据面）
构建优先:
- `/v1/chat/completions` + `/v1/embeddings`
- OpenAI 兼容的请求/响应面
- Providers: OpenAI, Azure OpenAI, Anthropic
- Auth: **Virtual Key**（AI Gateway 只处理 Virtual Key；JWT / IP Filter 等外围认证由前置 API Gateway 负责）
- 路由: 固定查找（model name → provider 直查，1:1 映射）
- 限流: RPM/TPM/并发（Redis）
- 缓存: 内存 + Redis
- 可观测: tracing + Prometheus + OTEL
- etcd 配置源 + 热加载
- 基础 Spend 追踪：记录请求 token 用量（input/output tokens）
- 响应头：`x-aisix-provider`、`x-aisix-cache-hit`
- **最小化 Admin HTTP API**：Provider / Model / Virtual Key 的 CRUD，默认内嵌于 aisix-gateway（开箱即用）。支持通过配置开关控制：开发/测试阶段启用内嵌 Admin API；生产环境可关闭 Admin API 或通过独立部署控制面（aisix-admin）来管理，以满足安全和隔离需求
- `/health`（liveness）+ `/ready`（readiness）HTTP 健康检查端点（复用 metrics 端口），确保 Kubernetes liveness/readiness probe 可用
- etcd 运行时断连重连机制：watch 连接断开后自动重试重连，重连期间使用旧快照继续提供服务

**预计工期：4-6 周**

> **开发/测试环境**：使用单实例 etcd 即可，无需集群部署。

### Phase 2 — 生产基线
构建优先:
- **Model 多 Deployment 数据模型**：model group / deployments 设计（见第十五章 TODO #2），落地后支持：
  - 路由策略：simple-shuffle / least-busy / weighted
  - 首字节前 Fallback（backup deployment list）
- Budget 层级（Global→Key→Customer；Team/Member 见第十五章 TODO #1）
- Per-Model 限流
- Provider 健康检查 + Cooldown
- **Spend 计费价格设计**（见第十五章 TODO #3）：定价表来源确定后，补全 cost 计算
- 响应头增强：`x-aisix-cost`、`x-aisix-remaining-budget`
- Latency-based / Usage-based 路由
- Callback sinks（Langfuse, Datadog）
- Request Mutation / Prompt Template

**预计工期：4-6 周**

### Phase 3 — 企业级
构建优先:
- Guardrail HTTP callback 引擎（pre/during/post-call）
- 密钥后端: AWS KMS / Vault / Azure Key Vault
- 语义缓存（Qdrant）
- 更多 Provider 适配器
- Admin 只读检查端点
- 告警（Slack/Email）
- Responses API / Images / Audio
- **优雅关闭**（Graceful Shutdown）：SIGTERM 信号处理 + 停止接收新请求 + 等待 in-flight 请求完成 + 排空异步任务管道

**预计工期：4-8 周**

### Phase 4 — 高级特性（按需）
构建优先:
- Realtime WebSocket API
- MCP/A2A Gateway
- 独立控制面服务
- 多区域灰度发布
- Provider 数量增加后 Feature-gate 拆分子 crate

**预计工期：8+ 周**

---

## 十二、风险与注意事项

| 风险 | 缓解措施 |
|------|---------|
| Redis 成为瓶颈 | Lua 脚本批量操作 + 本地影子限流减少 Redis 调用 |
| 流式重试语义复杂 | 首字节前可重试/降级，首字节后不再 fallback |
| Provider 归一化膨胀 | 严格编解码器契约 + 统一错误分类学，早期建立 |
| 配置变更 CPU 尖峰 | 去抖窗口 + 最小重建间隔 + 后台编译任务 |
| Provider 编译变慢 | Feature-gate 拆分子 crate |
| 配置验证错误不友好 | YAML path + JSON pointer + 人类可读消息 + 模糊修复建议 |

---

## 十三、参考项目

| 项目 | 说明 |
|------|------|
| LiteLLM | Python 开源 OpenAI 兼容 AI Gateway |
| Helicone/ai-gateway | Rust 生产的 AI Gateway |
| Grob | Rust LLM 路由代理 + DLP + 多 provider failover |
| ModelMux | Rust Vertex AI-OpenAI 代理 |
| CloakPipe | Rust PII 隐私代理（axum） |
| Apache APISIX | 高性能 API Gateway（参考其架构模式） |

---

## 十四、验收测试用例

> **覆盖范围**：本章用例覆盖第十一章 **Phase 1 — MVP（核心数据面）** 的全部交付项，包括：
> `/v1/chat/completions` + `/v1/embeddings`、Virtual Key 鉴权、RPM/TPM/并发限流（Redis）、
> 模型路由、流式代理、基础 Spend 计费、etcd 热加载。
> Phase 2/3 的功能（Guardrail、语义缓存、Budget 层级等）不在本章范围内。
>
> **格式说明**：测试用例均为 Rust 集成测试（`#[tokio::test]`），可在 CI 中自动执行。
> 外部依赖通过 wiremock（Mock HTTP Server）模拟上游 Provider，etcd/Redis 通过 docker-compose 提供。
> 所有测试共享一个 `TestApp::start()` 辅助函数，该函数：
> 1. 启动内嵌 etcd（或连接 docker-compose etcd）
> 2. 启动内嵌 Redis（或连接 docker-compose Redis）
> 3. 写入测试所需的 etcd 配置（provider、model、apikey、policy）
> 4. 启动 aisix-gateway 并等待健康检查通过
> 5. 返回 `TestApp { client, base_url, mock_server, etcd_client }`

### TC-01：有效 API Key 鉴权通过

```rust
/// 验证：合法 Bearer token → 请求被转发，返回 200
#[tokio::test]
async fn test_valid_api_key_passes() {
    let app = TestApp::start().await;
    // etcd 中已注册 key "sk-valid-key" → key_id "key-001"
    // mock upstream 返回正常 chat completion 响应

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["choices"][0]["message"]["content"].is_string());
}
```

### TC-02：无效 API Key → 401

```rust
/// 验证：不存在的 Bearer token → 立即返回 401，不转发上游
#[tokio::test]
async fn test_invalid_api_key_rejected() {
    let app = TestApp::start().await;

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-nonexistent")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "authentication_error");
    // mock upstream 没有收到任何请求
    app.mock_server.assert_no_pending_requests().await;
}
```

### TC-03：缺少 Authorization 头 → 401

```rust
/// 验证：请求不携带 Authorization 头 → 401
#[tokio::test]
async fn test_missing_api_key_rejected() {
    let app = TestApp::start().await;

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        // 故意不设置 bearer_auth
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 401);
}
```

### TC-04：内联 RPM 限流触发 → 429

```rust
/// 验证：apikey 内联 rate_limit.rpm=2，第 3 次请求返回 429
#[tokio::test]
async fn test_rate_limit_inline_enforced() {
    let app = TestApp::start_with_config(ApiKeyConfig {
        key: "sk-limited",
        rate_limit: Some(InlineRateLimit { rpm: 2, ..Default::default() }),
        policy_id: None,
        ..Default::default()
    }).await;

    for i in 0..2 {
        let resp = app.chat("sk-limited", "gpt-4o-mini").await;
        assert_eq!(resp.status(), 200, "request {i} should pass");
    }

    let resp = app.chat("sk-limited", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 429, "3rd request should be rate limited");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "rate_limit_error");
}
```

### TC-05：policy_id 限流触发 → 429

```rust
/// 验证：apikey 引用 policy_id，policy rpm=2，第 3 次请求返回 429
#[tokio::test]
async fn test_rate_limit_policy_id_enforced() {
    let app = TestApp::start_with_config(ApiKeyConfig {
        key: "sk-policy-limited",
        rate_limit: None,
        policy_id: Some("strict-tier"),  // strict-tier: rpm=2
        ..Default::default()
    }).await;

    for i in 0..2 {
        let resp = app.chat("sk-policy-limited", "gpt-4o-mini").await;
        assert_eq!(resp.status(), 200, "request {i} should pass");
    }

    let resp = app.chat("sk-policy-limited", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 429);
}
```

### TC-06：内联 rate_limit 覆盖 policy_id（inline 优先）

```rust
/// 验证：同时设置 policy_id(rpm=2) 和 inline rate_limit(rpm=10)
///      以 inline 为准，前 10 次请求应通过，第 11 次才被拒绝
#[tokio::test]
async fn test_inline_overrides_policy_id() {
    let app = TestApp::start_with_config(ApiKeyConfig {
        key: "sk-inline-wins",
        rate_limit: Some(InlineRateLimit { rpm: 10, ..Default::default() }),
        policy_id: Some("strict-tier"),  // strict-tier: rpm=2（应被覆盖）
        ..Default::default()
    }).await;

    // 前 10 次应全部通过（inline rpm=10 生效）
    for i in 0..10 {
        let resp = app.chat("sk-inline-wins", "gpt-4o-mini").await;
        assert_eq!(resp.status(), 200, "request {i} should pass with inline limit");
    }

    // 第 11 次超过 inline rpm=10 限制
    let resp = app.chat("sk-inline-wins", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 429);
}
```

### TC-07：Chat 非流式代理成功

```rust
/// 验证：/v1/chat/completions 非流式请求，上游正常响应，gateway 原样转发
#[tokio::test]
async fn test_chat_completion_proxy_non_streaming() {
    let app = TestApp::start().await;
    // mock upstream 返回标准 OpenAI chat completion JSON

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Say hello"}],
            "stream": false
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["usage"]["prompt_tokens"].is_number());
    assert!(body["usage"]["completion_tokens"].is_number());
}
```

### TC-08：Embeddings 端点代理成功

```rust
/// 验证：/v1/embeddings 请求被正确路由并代理，返回 embeddings 数组
#[tokio::test]
async fn test_embeddings_proxy() {
    let app = TestApp::start().await;
    // etcd 中已注册支持 embeddings 的 model "text-embedding-3-small"
    // mock upstream 返回标准 embeddings 响应

    let resp = app.client
        .post(format!("{}/v1/embeddings", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "text-embedding-3-small",
            "input": "Hello world"
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "list");
    assert!(body["data"][0]["embedding"].is_array());
    assert!(body["data"][0]["embedding"].as_array().unwrap().len() > 0);
}
```

### TC-09：Chat 流式代理 — SSE 事件格式正确

```rust
/// 验证：stream=true 时，gateway 输出合法的 OpenAI SSE 格式
///      包含 data: {...} 行，以 data: [DONE] 结束
#[tokio::test]
async fn test_chat_completion_proxy_streaming() {
    let app = TestApp::start().await;
    // mock upstream 返回 SSE 流：多个 delta chunk + [DONE]

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Count 1 to 3"}],
            "stream": true
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let text = resp.text().await.unwrap();
    // 至少有一个 data: {...} 行
    assert!(text.contains("data: {"));
    // 最后以 data: [DONE] 结束
    assert!(text.trim_end().ends_with("data: [DONE]"));

    // 解析所有非 [DONE] 的 data 行，验证 JSON 结构合法
    for line in text.lines() {
        if let Some(json_str) = line.strip_prefix("data: ") {
            if json_str == "[DONE]" { continue; }
            let chunk: serde_json::Value = serde_json::from_str(json_str).unwrap();
            assert_eq!(chunk["object"], "chat.completion.chunk");
        }
    }
}
```

### TC-10：流式结束后 usage 被记录

```rust
/// 验证：流式请求完成后，PostCall 阶段将 usage 写入 Redis 计数器
///      通过查询 Redis key 确认 token 消耗已扣除
#[tokio::test]
async fn test_streaming_usage_tracked() {
    let app = TestApp::start().await;
    // mock upstream 流中包含 StreamEvent::Usage (input=10, output=20)

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // 消费完整个流
    let _ = resp.text().await.unwrap();

    // 等待 PostCall 异步任务完成
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 查询 Redis，验证 token 计数已扣除
    let tpm_used: i64 = app.redis.get(format!(
        "rl:tpm:key:key-001:gpt-4o-mini:{}",
        current_minute_window()
    )).await;
    assert!(tpm_used >= 30, "expected at least 30 tokens (10 in + 20 out)");
}
```

### TC-11：按 key 路由到指定 provider

```rust
/// 验证：model 配置关联到特定 provider，请求被路由到正确的上游 URL
#[tokio::test]
async fn test_model_routing_by_provider() {
    let app = TestApp::start_with_two_providers().await;
    // provider-a mock: https://mock-a.local
    // provider-b mock: https://mock-b.local
    // model "model-a" → provider-a；model "model-b" → provider-b

    app.chat_to_model("sk-valid-key", "model-a").await;
    app.mock_provider_a.assert_hit_count(1).await;
    app.mock_provider_b.assert_hit_count(0).await;

    app.chat_to_model("sk-valid-key", "model-b").await;
    app.mock_provider_a.assert_hit_count(1).await;  // 不变
    app.mock_provider_b.assert_hit_count(1).await;
}
```

### TC-12：上游返回 5xx → gateway 返回 502

```rust
/// 验证：上游返回 500，gateway 归一化为 502 并返回 OpenAI 兼容错误体
#[tokio::test]
async fn test_upstream_5xx_returns_502() {
    let app = TestApp::start_with_failing_upstream(500).await;

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send().await.unwrap();

    assert_eq!(resp.status(), 502);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "upstream_error");
    // 不暴露上游内部错误细节
    assert!(body["error"].get("upstream_status").is_none());
}
```

### TC-13：spend 超限后请求被拒绝

```rust
/// 验证：key 的 max_budget 已耗尽，新请求返回 429（budget exceeded）
#[tokio::test]
async fn test_spend_limit_blocks_after_exceeded() {
    let app = TestApp::start_with_config(ApiKeyConfig {
        key: "sk-budget-key",
        max_budget_usd: Some(0.000001),  // 极小预算，立刻耗尽
        ..Default::default()
    }).await;

    // 第一次请求（usage 足以耗尽预算）
    let _ = app.chat("sk-budget-key", "gpt-4o-mini").await;

    // 等待 PostCall 异步记账完成
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 第二次请求应被预算检查拒绝
    let resp = app.chat("sk-budget-key", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 429);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "budget_exceeded");
}
```

### TC-14：热更新 rate_limit 后立即生效

```rust
/// 验证：通过 etcd 写入新的 rate_limit 配置，热加载后新限制立即生效
#[tokio::test]
async fn test_hot_reload_rate_limit_change() {
    let app = TestApp::start_with_config(ApiKeyConfig {
        key: "sk-reload-key",
        rate_limit: Some(InlineRateLimit { rpm: 100, ..Default::default() }),
        ..Default::default()
    }).await;

    // 初始：rpm=100，请求应通过
    let resp = app.chat("sk-reload-key", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 200);

    // 通过 etcd 将 rpm 改为 0（锁定该 key）
    app.etcd_client.put(
        "/aisix/apikeys/key-reload",
        r#"{"id":"key-reload","key":"sk-reload-key","rate_limit":{"rpm":0}}"#,
    ).await.unwrap();

    // 等待热加载完成（debounce ~500ms + 编译时间）
    tokio::time::sleep(Duration::from_millis(800)).await;

    // 新配置生效：rpm=0，请求应被拒绝
    let resp = app.chat("sk-reload-key", "gpt-4o-mini").await;
    assert_eq!(resp.status(), 429);
}
```

### TC-15：etcd 启动时不可用 — gateway 拒绝启动

```rust
/// 验证：etcd 不可用时，gateway 启动失败并退出（而非以空配置提供服务）
#[tokio::test]
async fn test_etcd_unavailable_at_startup_fails() {
    // 不启动 etcd，直接尝试启动 gateway
    let result = TestApp::start_without_etcd().await;

    // 预期：启动失败，返回错误
    assert!(result.is_err(), "gateway should fail to start without etcd");
    // 不应以空配置提供任何服务（安全 fail-fast 原则）
}
```

### TC-16：Redis 故障时请求放行（降级为仅本地限流）

```rust
/// 验证：Redis 不可用时，限流检查降级为仅本地 shadow 限流器，请求仍被转发
///      本地 shadow 限流器（in-memory GCRA）继续工作，精度降低但仍有基本保护
///      这是"可用性优先"策略：宁可降级，不因 Redis 故障拒绝所有请求
#[tokio::test]
async fn test_redis_failure_degrade_to_local() {
    let app = TestApp::start_with_redis_down().await;
    // mock upstream 仍正常响应

    let resp = app.client
        .post(format!("{}/v1/chat/completions", app.base_url))
        .bearer_auth("sk-valid-key")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send().await.unwrap();

    // Redis 故障时请求应被放行（返回 200），降级为仅本地限流，而非 500/429
    assert_eq!(resp.status(), 200);
}
```

### 错误响应格式规范

所有错误响应统一为 OpenAI 兼容格式，HTTP 状态码与 `error.type` 的对应关系：

| HTTP 状态码 | `error.type` | 触发场景 |
|------------|-------------|---------|
| 401 | `authentication_error` | API Key 无效或缺失 |
| 403 | `permission_denied` | Key 无权访问该 model |
| 429 | `rate_limit_error` | RPM/TPM/并发超限 |
| 429 | `budget_exceeded` | spend 预算耗尽 |
| 400 | `invalid_request_error` | 请求体格式错误 |
| 400 | `context_length_exceeded` | 输入 token 数超过模型上下文窗口限制 |
| 400 | `content_filter_error` | 上游内容安全策略拦截（provider 返回内容过滤错误） |
| 502 | `upstream_error` | 上游 5xx 或连接失败 |
| 504 | `timeout_error` | 上游超时 |
| 500 | `internal_error` | gateway 内部错误 |

```json
{
  "error": {
    "message": "Rate limit exceeded: rpm limit 100 for key key-abc123",
    "type": "rate_limit_error",
    "code": "rate_limit_exceeded"
  }
}
```

### 超时参数参考

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `upstream.connect_timeout_ms` | 5000 | 建立 TCP 连接超时 |
| `upstream.request_timeout_ms` | 60000 | 首字节超时（非流式） |
| `upstream.stream_idle_timeout_ms` | 120000 | 流式传输中无数据超时 |
| `upstream.guardrail_timeout_ms` | 3000 | Guardrail HTTP callback 超时 |

---

## 十五、Future TODO（暂不设计，留待后续迭代）

以下功能点经评估暂不纳入当前设计，但已在相关章节有所引用。待积累足够生产经验后，逐步补充详细设计。

### 1. Team / Team Member 预算层级

**背景**：当前预算层级为 `Global → Key → Customer`。Team/TeamMember 组织层级存在于 LiteLLM 设计中，但 AISIX MVP 阶段无需引入。

**待设计**：
- etcd 数据模型中 Team 实体的结构（team_id、成员列表、预算字段）
- `KeyMeta` 添加 `team_id` 字段后的策略合并逻辑
- `rl:budget:team:{id}` Redis key 的计费聚合方式
- Phase 2 中五层层级（Global → Team → Member → Key → Customer）的优先级规则

---

### 2. Model 多 Deployment 路由 / Fallback

**背景**：当前模型 1:1 映射一个 provider 配置，无法支持同一逻辑模型对应多个 deployment 的负载均衡或 fallback。**已分配至 Phase 2，Phase 2 动工前需完成以下设计。**

**待设计**:
- `ModelConfig` 中 `deployments: Vec<DeploymentConfig>` 字段设计
- 路由策略枚举：round-robin、least-latency、weighted、priority-fallback
- 跨 deployment 的健康检查与 cooldown 共享方式
- etcd 中 model group 数据模型（逻辑模型名 → deployment 列表）

---

### 3. Spend 计费价格数据来源

**背景**：当前 `aisix-spend` crate 负责费用计算，但 token 单价数据来源未设计。Phase 1 仅记录 token 用量，cost 计算推迟到 Phase 2。**已分配至 Phase 2，Phase 2 动工前需完成以下设计。**

**待设计**:
- 价格数据存储位置：etcd 静态配置 vs 控制面 API 下发 vs 内置默认表
- 价格更新机制：Provider 调价时如何热更新
- 自定义计费（企业自定义溢价/折扣）的配置格式
- 货币单位与精度处理（微美元、整数 vs 浮点）

---

### 4. API Key 安全存储

**背景**：MVP 阶段 Virtual Key 明文存储在 etcd 中，以 Bearer token 作为 HashMap key 直接查询。

**待设计**：
- Hash 存储方案：BLAKE3/SHA256 对 Key 哈希后存储，查询时对输入 hash 后再比对
- 密钥后端集成：AWS KMS / HashiCorp Vault / Azure Key Vault（Phase 3）
- Key rotation 流程：新老 Key 并行有效期、轮换通知

---

### 5. 超时参数配置归属

**背景**：第十四章超时参数表列出了四个超时配置，但未明确其在 etcd 数据模型中的归属层级。

**待设计**：
- 超时参数是全局配置、Provider 级配置还是 Key 级配置
- 各层级覆盖优先级（Key 级 > Provider 级 > 全局默认）
- 与 etcd `GlobalConfig` / `ProviderConfig` / `KeyMeta` 结构体的绑定关系

---

### 6. 健康检查端点（/health、/ready）

**背景**：Phase 1 的 aisix-server 在代码注释中提及 health/metrics endpoints，但 Phase 1 交付列表中未显式列出，也无专项设计。Kubernetes liveness/readiness probe 和负载均衡器的接入依赖这两个端点。**已分配至 Phase 2，Phase 2 动工前需完成以下设计。**

**待设计**:
- `/health`（liveness）：检查 gateway 进程本身是否存活（返回 200 即可）
- `/ready`（readiness）：检查 gateway 是否已完成启动（etcd 快照已加载、Redis 可达）
- readiness 检查的依赖范围：etcd 连接、Redis 连接、初始快照编译是否纳入
- 探针失败时的 HTTP 状态码（503）及响应体格式
- 是否与 `/metrics`（Prometheus）共用端口或单独绑定

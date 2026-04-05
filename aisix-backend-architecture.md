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
| **配置同步** | etcd 协议（支持真 etcd 或 DP Manager mock） | 数据面只依赖 etcd 协议，部署灵活，watch 语义可靠 |
| **分发策略** | 热路径用枚举静态分发，扩展点用 HTTP callback | 保留 Rust 最大性能优势，无需动态插件系统 |
| **状态模型** | 不可变编译快照 + ArcSwap 原子切换 | 配置热加载零停机，无读写锁竞争 |
| **流式代理** | 零拷贝直通 + 按需转码 | OpenAI 兼容上游直接隧道，非兼容上游才转码 |
| **Guardrails** | 内置 HTTP callback 引擎 | 不引入插件系统，guardrail 即外部 HTTP 服务调用 |
| **扩展方式** | 内置服务 + 外部 HTTP callback | 编译时类型安全，不牺牲性能 |

---

## 二、总体架构图

### 配置同步双模式

**数据面只感知 etcd 协议，不感知背后是真正的 etcd 还是 PostgreSQL + DP Manager。**

```
模式 A（纯 etcd 部署）：

  Control Plane ──▶ etcd ──────────────▶ AISIX Data Plane
                   (真正的 etcd 集群)      (etcd watch)


模式 B（PG + DP Manager 部署）：

  Control Plane ──▶ PostgreSQL ──▶ DP Manager ──▶ AISIX Data Plane
                   (真相源)         (etcd mock)     (etcd watch)
                                    转译 PG 数据
                                    为 etcd 协议
```

### 整体架构

```
                    ┌──────────────────────────────────────────┐
                    │            Control Plane                  │
                    │  CLI / Admin API / Dashboard / Ingress    │
                    └─────────┬──────────────┬─────────────────┘
                              │              │
                    ┌─────────▼──────┐  ┌────▼─────────────┐
                    │   PostgreSQL    │  │    etcd 集群      │
                    │   (模式B真相源) │  │   (模式A直接)     │
                    └─────────┬──────┘  └────┬─────────────┘
                              │              │
                    ┌─────────▼──────┐       │
                    │  DP Manager    │       │
                    │  (etcd mock)   │───────┤
                    │  PG → etcd协议 │       │
                    └────────────────┘       │
                                             │
                 ┌───────────────────────────┘
                 │ etcd watch (统一协议)
                 │
    ┌────────────┼────────────────┐
    │            │                │
    ▼            ▼                ▼
┌─────────┐ ┌─────────┐   ┌─────────┐
│ Node A  │ │ Node B  │   │ Node C  │
│ aisixd  │ │ aisixd  │   │ aisixd  │
│         │ │         │   │         │
│ etcd    │ │ etcd    │   │ etcd    │
│ watcher │ │ watcher │   │ watcher │
│    ↓    │ │    ↓    │   │    ↓    │
│ Compiled│ │ Compiled│   │ Compiled│
│ Snapshot│ │ Snapshot│   │ Snapshot│
└─────────┘ └─────────┘   └─────────┘
```

### 数据面节点内部结构

```
┌──────────────────────────────────────────────────────────┐
│                    AISIX Data Plane Node                  │
│                                                          │
│  ┌────────────────┐   ┌──────────────────────────────┐  │
│  │ etcd Watcher   │──▶│ Arc<CompiledSnapshot>        │  │
│  │ (config sync)  │   │ (不可变配置，ArcSwap 原子切换) │  │
│  └────────────────┘   └──────────────┬───────────────┘  │
│                                      │                   │
│  ┌───────────────────────────────────▼────────────────┐ │
│  │              Request Pipeline (axum/tower)          │ │
│  │                                                     │ │
│  │  Auth → Policy → Mutation → Guardrail → RateLimit  │ │
│  │  → Cache → Router → Provider → Guardrail → Spend   │ │
│  │  → Logging → Response                               │ │
│  └──────────────────────────┬─────────────────────────┘ │
│                             │                            │
│  ┌──────────────────────────▼─────────────────────────┐ │
│  │           Upstream Pool (hyper client)              │ │
│  │  连接池 (scheme+host+port) / HTTP2 / Keepalive      │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─────────────────┐  ┌─────────────┐  ┌─────────────┐ │
│  │ Redis Client    │  │ PG Client   │  │ Background  │ │
│  │ (限流/缓存/并发) │  │ (ledger)    │  │ Tasks       │ │
│  └─────────────────┘  └─────────────┘  │ - spend flush│ │
│                                         │ - health chk │ │
│                                         │ - metrics    │ │
│                                         └─────────────┘ │
└──────────────────────────────────────────────────────────┘
```

### 为什么选择 etcd 协议

| 维度 | PG + Redis pub/sub | etcd 协议统一（选择） |
|------|---|---|
| **数据面复杂度** | 需要同时对接 PG + Redis + 文件 | 只对接 etcd，单一协议 |
| **部署灵活性** | 绑定 PG + Redis | 可选真 etcd 或 PG + DP Manager |
| **APISIX 生态兼容** | 不兼容 | 天然兼容 APISIX 控制面 |
| **Watch 语义** | Redis pub/sub 不可靠，需自实现 | etcd watch 原生支持，revision 保证不丢事件 |
| **多节点一致性** | 需要自己保证 | etcd MVCC 天然保证 |
| **配置回滚** | 需要自己实现 | etcd 历史版本天然支持 |

---

## 三、请求处理管线

### 完整请求路径

```
Client Request
  │
  ▼
[1. HTTP Ingress] ─── axum 路由匹配 /v1/chat/completions, /v1/embeddings, ...
  │
  ▼
[2. Decode + Normalize] ─── 反序列化为 CanonicalRequest（统一内部类型）
  │
  ▼
[3. RequestContext 创建] ─── request_id + trace span + Exchange 对象
  │
  ▼
[4. Authentication] ─── Virtual Key / JWT / IP Filter
  │
  ▼
[5. Policy Resolution] ─── 解析 Key→Team→Member→Customer 层级，确定有效策略
  │                     ─── 允许的模型、标签、参数、限额
  │
  ▼
[6. Request Mutation] ─── prompt template 应用
  │                   ─── drop params / modify params
  │                   ─── enforce user param / size check
  │
  ▼
[7. Pre-Call Guardrails] ─── 并发 HTTP callback（PII 脱敏、内容安全）
  │                       ─── 可阻塞/转换/注解请求
  │
  ▼
[8. Rate Limit + Budget Precheck]
  │   ─── 本地影子限流器（快速拒绝明显超限）
  │   ─── Redis 权威限流器（RPM/TPM/并发/预算）
  │   ─── TPM: 预估 input + max_output token，预留额度
  │
  ▼
[9. Cache Lookup] ─── 内存/Redis 缓存命中？
  │
  ├── hit ──▶ [响应归一化] ──▶ [返回缓存响应]
  │
  ▼ miss
[10. Routing] ─── model group 解析（别名/通配符/标签匹配）
  │           ─── 策略选择（simple-shuffle/least-busy/latency/usage）
  │           ─── fallback 计划生成
  │           ─── 冷却排除
  │
  ▼
[11. Provider Request Build] ─── 编解码器构建上游 HTTP 请求
  │
  ▼
[12. Upstream Call] ─── 超时控制
  │                 ─── 首字节前可重试/降级
  │
  ├── 非流式分支 ──────────────────────────────────────────────┐
  │    ▼                                                        │
  │  [完整响应解析]                                               │
  │    ▼                                                        │
  │  [Post-Call Guardrails] ─── HTTP callback                    │
  │    ▼                                                        │
  │  [Usage/Cost 提取]                                           │
  │    ▼                                                        │
  │  [缓存写入（可选）]                                           │
  │    ▼                                                        │
  │  [异步 Spend/Logging] ─── 批量写入 PG + callback sink        │
  │    ▼                                                        │
  │  [返回 JSON 响应]                                            │
  │                                                              │
  └── 流式分支 ──────────────────────────────────────────────────┐
       ▼                                                        │
     [Stream Transcoder] ─── 上游 SSE 格式 → OpenAI SSE          │
       ▼                   (上游已兼容则零拷贝直通)                 │
     [During-Stream Guardrails] ─── 仅超时可控的 HTTP callback     │
       ▼                                                        │
     [增量 Usage 统计] ─── 累计 token 计数                         │
       ▼                                                        │
     [客户端 SSE 流]                                              │
       ▼                                                        │
     [流结束 → 异步结算/日志]                                      │
```

### 固定执行阶段

| 阶段 | 名称 | 职责 |
|------|------|------|
| 1 | Decode | 反序列化 + normalize |
| 2 | Auth | 身份验证 |
| 3 | Policy | 策略解析 |
| 4 | PreCall | Guardrail pre-call |
| 5 | RateLimit | 限流 + 预算检查 |
| 6 | CacheLookup | 缓存查询 |
| 7 | RouteSelect | 路由选择 |
| 8 | PreUpstream | 请求构建 |
| 9 | UpstreamHeaders | 上游响应头处理 |
| 10 | StreamChunk | 流式 chunk 处理 |
| 11 | PostCall | Guardrail post-call + usage |
| 12 | OnError | 错误处理 |

### 流式 vs 非流式关键区别

**非流式：**
- 读取完整上游响应体
- 解析为 canonical response
- 应用 post-call guardrails
- 提取 usage/cost
- 可选缓存
- 返回 JSON

**流式：**
- **首字节前**可以重试/fallback
- **首字节后**不再重试，该 provider 调用确定
- 适配上游流为 OpenAI-style SSE
- 增量更新 token/usage
- 流结束时异步结算

**核心原则：零拷贝直通**
- 如果上游本身就是 OpenAI 兼容 SSE 且无 guardrail 需要修改 chunk → 直接隧道转发 `Bytes`
- 否则 → 运行一次性转码器

---

## 四、Cargo Workspace 结构

### 目录布局

```
aisix/
├── Cargo.toml                    # workspace 根
├── bin/
│   └── aisixd/                   # 可执行入口
│       └── main.rs
└── crates/
    │
    │  ── 基础层 ──
    ├── aisix-types/              # 共享类型：CanonicalRequest/Response, Usage, IDs, Error
    ├── aisix-core/               # 核心抽象：Exchange, GatewayState, 管线编排
    ├── aisix-config/             # etcd watch + 编译快照 + 验证 + 热加载
    │
    │  ── 存储层 ──
    ├── aisix-storage/            # PG(ledger/audit) + Redis(计数器/缓存) + 密钥解析
    │
    │  ── 领域模块 ──
    ├── aisix-auth/               # Virtual Key, JWT, IP Filter
    ├── aisix-policy/             # 层级策略解析, 模型/标签访问控制, 参数变异
    ├── aisix-router/             # model group 解析, 路由策略, fallback, cooldown
    ├── aisix-ratelimit/          # RPM/TPM/并发/预算检查, 本地影子 + Redis 权威
    ├── aisix-cache/              # 内存/Redis/Disk/S3/语义缓存后端
    ├── aisix-providers/          # Provider 编解码器, 请求/响应转换, 错误归一化
    ├── aisix-guardrail/          # HTTP callback 引擎, pre/post-call guardrail
    ├── aisix-spend/              # 定价, 费用计算, 异步批量记账, 预算对账
    ├── aisix-observability/      # tracing, Prometheus, OTEL, callback sink fanout
    │
    │  ── 编排层 ──
    ├── aisix-runtime/            # 运行时组合, 后台任务, 快照生命周期, 注册表
    └── aisix-server/             # axum 路由, 请求提取, SSE 响应, health/metrics 端点
```

### Crate 职责表

| Crate | 职责 | 拥有的状态/数据 |
|-------|------|----------------|
| `aisix-types` | 共享类型定义 | CanonicalRequest/Response, Usage, IDs, Error 枚举 |
| `aisix-core` | 核心抽象 | Exchange, GatewayState, 管线编排逻辑 |
| `aisix-config` | 配置系统 | CompiledSnapshot, etcd watcher, 验证器 |
| `aisix-storage` | 持久化 | PG repo, Redis repo, 密钥解析器 |
| `aisix-auth` | 认证 | Principal 解析, Key 校验 |
| `aisix-policy` | 策略 | EffectivePolicy 合并, 访问决策 |
| `aisix-router` | 路由 | RouteDecision, 策略实现, 健康状态 |
| `aisix-ratelimit` | 限流 | RateDecision, 本地计数器 |
| `aisix-cache` | 缓存 | CacheBackend 实现, key builder |
| `aisix-providers` | Provider | 编解码器, 流式适配器, 错误映射 |
| `aisix-guardrail` | 安全护栏 | HTTP callback 引擎, 超时控制 |
| `aisix-spend` | 费用 | UsageEvent, 定价表, 批量管道 |
| `aisix-observability` | 可观测 | Metrics, tracing spans, callback sink |
| `aisix-runtime` | 运行时 | 后台任务注册, 快照生命周期 |
| `aisix-server` | HTTP 服务 | axum 路由, 中间件, SSE 处理 |

### 依赖关系图

```
aisix-types
    ↑
aisix-core ←── aisix-config
    ↑               ↑
    ├── aisix-auth   |
    ├── aisix-policy |
    ├── aisix-router |
    ├── aisix-ratelimit
    ├── aisix-cache
    ├── aisix-providers
    ├── aisix-guardrail
    ├── aisix-spend
    └── aisix-observability

aisix-storage → aisix-types + aisix-core
aisix-runtime → all domain crates
aisix-server → aisix-runtime + aisix-observability
aisixd → aisix-server
```

---

## 五、核心 Trait 系统

### 设计原则

**热路径用枚举静态分发（零开销），扩展点用 HTTP callback（无 trait object 开销）。**

不做 `dyn Trait` 动态分发作为默认行为——这是 AISIX 与 litellm 的根本区别。

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

pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: bool,
    pub cache_write: bool,
    pub cost_micros_usd: u64,
}
```

### 请求上下文

```rust
// ===== aisix-core =====

/// 贯穿整个管线的状态容器
pub struct Exchange {
    pub request_id: Uuid,
    pub started_at: Instant,
    pub operation: Operation,
    pub request: CanonicalRequest,
    pub principal: Option<Principal>,          // 阶段 4 填充
    pub policy: Option<Arc<EffectivePolicy>>,  // 阶段 5 填充
    pub route: Option<RouteDecision>,          // 阶段 10 填充
    pub usage: UsageAccumulator,               // 流式增量累计
    pub labels: RequestLabels,                 // tags, trace info
    pub streaming: bool,
}
```

### Provider 编解码器

```rust
/// 每个 LLM Provider 实现此 trait，负责请求/响应格式转换
#[async_trait]
pub trait ProviderCodec: Send + Sync + 'static {
    fn kind(&self) -> ProviderKind;
    fn capabilities(&self) -> ProviderCapabilities;

    /// 将 CanonicalRequest 构建为上游 HTTP 请求
    fn build_request(
        &self,
        ex: &Exchange,
        target: &ResolvedTarget,
    ) -> Result<http::Request<Body>, GatewayError>;

    /// 解析上游 HTTP 响应为统一输出
    async fn parse_response(
        &self,
        ex: &Exchange,
        target: &ResolvedTarget,
        resp: http::Response<Incoming>,
    ) -> Result<ProviderOutput, GatewayError>;

    /// 错误归一化
    fn normalize_error(&self, status: StatusCode, body: &[u8]) -> GatewayError;
}

pub enum ProviderOutput {
    Json {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
        usage: Option<Usage>,
    },
    Stream {
        status: StatusCode,
        headers: HeaderMap,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, GatewayError>> + Send>>,
    },
}
```

### 内置 Provider 枚举分发

```rust
/// 核心 Provider 用枚举静态分发（零开销）
/// 第三方 Provider 用 Dynamic 变体（唯一需要 dyn 的地方）
pub enum BuiltinProvider {
    OpenAI(OpenAIProvider),
    Anthropic(AnthropicProvider),
    AzureOpenAI(AzureOpenAIProvider),
    Vertex(VertexProvider),
    Bedrock(BedrockProvider),
    Ollama(OllamaProvider),
    Dynamic(Arc<dyn ProviderCodec>),  // 第三方扩展点
}

impl BuiltinProvider {
    pub async fn execute(
        &self,
        ex: &Exchange,
        target: &ResolvedTarget,
        client: &UpstreamClient,
    ) -> Result<ProviderOutput, GatewayError> {
        match self {
            Self::OpenAI(p) => client.execute_codec(p, ex, target).await,
            Self::Anthropic(p) => client.execute_codec(p, ex, target).await,
            Self::AzureOpenAI(p) => client.execute_codec(p, ex, target).await,
            Self::Vertex(p) => client.execute_codec(p, ex, target).await,
            Self::Bedrock(p) => client.execute_codec(p, ex, target).await,
            Self::Ollama(p) => client.execute_codec(p, ex, target).await,
            Self::Dynamic(p) => client.execute_codec(p.as_ref(), ex, target).await,
        }
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
        ex: &Exchange,
        limits: &ResolvedLimits,
    ) -> Result<RateDecision, GatewayError>;

    /// 请求后结算（实际 usage）
    async fn settle(
        &self,
        ex: &Exchange,
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

## 六、状态管理

### 四层存储模型

| 层级 | 存储位置 | 内容 | 访问模式 |
|------|---------|------|---------|
| **L1 热路径** | 进程内 `Arc<CompiledSnapshot>` | 路由索引、策略表、模板、正则、Provider 注册表 | 无锁读，ArcSwap 原子切换 |
| **L2 分布式计数** | Redis | RPM/TPM 计数器、并发租约、冷却标记、实时花费 | Lua 脚本原子操作 |
| **L3 共享缓存** | Redis / S3 / GCS | 响应缓存（非流式）、语义缓存向量 | 异步读写 |
| **L4 持久真相** | PostgreSQL | 使用量账本、审计日志、预算定义、定价表 | 异步批量写入 |

### 限流器两层模型

```
请求进入
  │
  ▼
[本地影子限流器] ─── 内存 EWMA 计数，极低成本
  │                    明显超限 → 直接拒绝（保护 Redis）
  │
  ▼ 通过
[Redis 权威限流器] ─── Lua 脚本原子检查 + 预留
  │
  ├── 拒绝 → 返回 429
  │
  ▼ 通过
[执行请求]
  │
  ▼
[异步结算] ─── 实际 usage 增量更新 Redis + 批量写入 PG
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
请求开始:
  预估 input_tokens + max_output_tokens
  预留额度到 Redis

请求完成:
  计算实际 usage
  结算差值到 Redis（多退少补）
```

### 并发租约（Redis Sorted Set）

```
请求开始:
  ZADD rl:cc:{scope}:{id} {expires_at} {request_id}
  ZREMRANGEBYSCORE rl:cc:{scope}:{id} 0 {now}   ← 清理过期
  ZCARD rl:cc:{scope}:{id} > limit → 拒绝

请求完成:
  ZREM rl:cc:{scope}:{id} {request_id}
```

### Spend 追踪：异步管道

**关键：请求线程永不阻塞在 PG 写入上。**

```
请求路径
  → 创建 UsageEvent
  → 发送到 bounded mpsc channel
  → 立即返回响应给客户端

后台批量处理器
  ← 从 channel 消费
  ├── 增量更新 Redis 实时花费计数器
  ├── 批量 INSERT PostgreSQL usage_events 表
  └── 批量 UPSERT 聚合汇总表
```

### 预算执行层级

```
Global Proxy Budget
  └─ Team Budget (+ per-model RPM/TPM)
       └─ Team Member Budget (max_budget_in_team)
            └─ Virtual Key Budget (+ per-model budget)
                 └─ Customer (end-user) Budget
```

每个层级独立检查，任一层级拒绝即返回 429。

---

## 七、Provider 适配器设计

### 三层架构

```
┌────────────────────────────────────┐
│  Canonical API Layer               │  AISIX 统一请求/响应类型
│  (aisix-types)                     │  CanonicalRequest / CanonicalResponse
├────────────────────────────────────┤
│  Provider Codec Layer              │  每个 Provider 一个编解码器
│  (aisix-providers)                 │  - OpenAICodec
│                                    │  - AnthropicCodec
│                                    │  - AzureCodec
│                                    │  - VertexCodec
│                                    │  - BedrockCodec
│                                    │  - OllamaCodec
│                                    │  - ... (100+)
├────────────────────────────────────┤
│  Shared Transport Layer            │  统一上游 HTTP 客户端
│  (hyper client pool)               │  - 连接池（按 scheme+host+port）
│                                    │  - HTTP/2 优先，HTTP/1.1 keepalive
│                                    │  - DNS 缓存
│                                    │  - per-origin 超时配置
└────────────────────────────────────┘
```

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
  → 应用 model-specific prompt template
  → drop/modify params（按策略）
  → 映射 logical model → provider deployment
  → ProviderCodec 构建 HTTP 请求
  → 发送到上游
```

### 流式适配

```
上游 Provider SSE 格式各异：
  OpenAI    → data: {"choices":[{"delta":{"content":"Hi"}}]}
  Anthropic → event: content_block_delta / data: {"delta":{"text":"Hi"}}
  Gemini    → JSON lines with candidates

         ↓ 统一转换为内部流 ↓

StreamEvent::Delta(Bytes)      // 内容块
StreamEvent::Usage(UsageDelta) // 增量 token
StreamEvent::Done              // 结束

         ↓ 渲染为 OpenAI 兼容 SSE ↓

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

// 每个归一化错误携带：
// - retryable: bool
// - provider_status: Option<u16>
// - provider_code: Option<String>
// - provider_request_id: Option<String>
//
// 重试和 fallback 逻辑基于归一化类型，不基于 provider 特定字符串
```

---

## 八、配置系统

### YAML Schema 示例

```yaml
version: 1

server:
  listen: "0.0.0.0:4000"
  request_body_limit_mb: 8
  response_body_limit_mb: 64
  force_ipv4: false

providers:
  - id: openai-us
    kind: openai
    base_url: "https://api.openai.com"
    auth:
      secret_ref: "env:OPENAI_API_KEY"
    models:
      - alias: "gpt-4o-mini"
        provider_model: "gpt-4o-mini"
        input_price_micros_usd: 150
        output_price_micros_usd: 600
        capabilities: [chat, responses, embeddings]

  - id: azure-eastus
    kind: azure_openai
    base_url: "https://myazure.openai.azure.com"
    auth:
      secret_ref: "vault:kv/azure-openai"
    models:
      - alias: "gpt-4o-mini"
        provider_model: "gpt-4o-mini-prod"

  - id: anthropic
    kind: anthropic
    base_url: "https://api.anthropic.com"
    auth:
      secret_ref: "env:ANTHROPIC_API_KEY"
    models:
      - alias: "claude-sonnet"
        provider_model: "claude-sonnet-4-20250514"
        input_price_micros_usd: 300
        output_price_micros_usd: 1500
        capabilities: [chat]

model_groups:
  - name: "default-fast-chat"
    match:
      aliases: ["gpt-4o-mini", "gpt-4o*"]
    strategy: "least_busy"
    fallbacks: ["default-cheap-chat"]
    routes:
      - target: "openai-us/gpt-4o-mini"
        weight: 70
        tags: ["primary", "us"]
        rpm: 200
        tpm: 200000
      - target: "azure-eastus/gpt-4o-mini"
        weight: 30
        tags: ["backup", "azure"]

  - name: "default-cheap-chat"
    match:
      aliases: ["gpt-3.5-turbo"]
    strategy: "simple_shuffle"
    routes:
      - target: "openai-us/gpt-3.5-turbo"
        weight: 100

auth:
  virtual_keys:
    enabled: true
  jwt:
    enabled: true
    jwks_url: "https://issuer/.well-known/jwks.json"
  ip_filter:
    allow_cidrs: ["10.0.0.0/8"]

limits:
  global:
    rpm: 100000
  by_key:
    default:
      rpm: 600
      tpm: 300000
      concurrent: 20
      monthly_budget_usd: 500

cache:
  default_backend: "redis"
  key_fields: ["model", "messages", "tools", "temperature"]
  backends:
    redis:
      url: "redis://redis:6379"
      ttl_seconds: 300

guardrails:
  - name: "pii-redact"
    mode: "pre_call"
    default_on: true
    endpoint: "https://guard.internal/pii/redact"
    timeout_ms: 200
    secret_ref: "env:GUARD_API_KEY"
  - name: "output-policy"
    mode: "post_call"
    endpoint: "https://guard.internal/content/check"
    timeout_ms: 500

observability:
  prometheus: true
  otel: true
  callbacks:
    - kind: langfuse
      endpoint: "https://langfuse.internal"
      secret_ref: "env:LANGFUSE_KEY"
    - kind: datadog
      endpoint: "https://datadog.internal"
      api_key_ref: "env:DD_API_KEY"
    - kind: webhook
      endpoint: "https://my-service/webhook"
      headers:
        Authorization: "Bearer {{secret:env:WEBHOOK_TOKEN}}"
```

### etcd 数据模型

```yaml
# etcd key 前缀设计
/aisix/
  ├── config/
  │   ├── version          # 全局配置版本号（单调递增）
  │   └── settings         # 全局设置（server, observability, health）
  │
  ├── providers/
  │   ├── {provider_id}    # 每个 provider 一个 key
  │   │                    # 值: JSON {id, kind, base_url, auth, models:[...]}
  │   └── ...
  │
  ├── models/
  │   ├── groups/
  │   │   ├── {group_name} # model group 定义 + routes
  │   │   │                # 值: JSON {name, match, strategy, fallbacks, routes:[...]}
  │   │   └── ...
  │   └── aliases/
  │       ├── {alias}      # alias → group_name 映射（用于快速查找）
  │       │                # 值: "default-fast-chat"
  │       └── ...
  │
  ├── auth/
  │   ├── keys/
  │   │   ├── {key_hash}   # Virtual Key 元数据（不含明文密钥）
  │   │   │                # 值: JSON {team_id, user_id, models:[...], limits:{...}}
  │   │   └── ...
  │   ├── jwt/
  │   │   └── config       # JWT 配置（jwks_url, issuer, audience）
  │   └── ip_filter        # IP 过滤规则
  │
  ├── policies/
  │   ├── global           # 全局策略
  │   ├── teams/
  │   │   ├── {team_id}    # Team 级策略
  │   │   └── ...
  │   └── defaults         # 默认策略
  │
  ├── limits/
  │   ├── global           # 全局限流配置
  │   ├── teams/
  │   │   └── {team_id}    # Team 级限流
  │   └── keys/
  │       └── {key_hash}   # Key 级限流
  │
  ├── cache/
  │   └── config           # 缓存后端配置
  │
  ├── guardrails/
  │   ├── {guardrail_name}  # Guardrail 定义
  │   │                      # 值: JSON {name, mode, endpoint, timeout_ms, ...}
  │   └── ...
  │
  └── health/
      ├── checks             # 健康检查配置
      └── cooldowns/
          └── {target_id}    # Cooldown 状态
```

### 配置编译流程

```
aisixd 启动
  │
  ▼
连接 etcd（或 DP Manager etcd mock）
  │
  ▼
[1] GET /aisix/config/version → 获取当前版本号
  │
  ▼
[2] GET /aisix/ prefix (range) → 拉取全量配置
  │
  ▼
[3] 解析密钥引用（env/KMS/Vault → 实际值）
  │
  ▼
[4] 三层验证
  │    ├── Schema 验证：必填字段、枚举值、类型正确性
  │    ├── 语义验证：引用完整性、无环 fallback、限额合理
  │    └── 运行时预检：URL 格式、auth 完整性、能力兼容
  │
  ▼
[5] 编译为 CompiledSnapshot
  │    ├── 构建路由索引（alias → group, wildcard matcher）
  │    ├── 构建策略查找表（key → EffectivePolicy）
  │    ├── 预编译正则/模板
  │    ├── 构建 Provider 注册表
  │    └── 构建 guardrail 链
  │
  ▼
[6] ArcSwap 原子切换 → 开始服务流量
  │
  ▼
[7] WATCH /aisix/ prefix (from revision N+1) → 持续监听变更
  │
  ├── 收到 PUT 事件
  │    ├── 去抖（250-500ms 合并窗口）
  │    ├── 合并变更到本地配置
  │    ├── 重新验证 + 编译
  │    └── ArcSwap 原子切换（验证失败则保留旧快照）
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
model_groups[1].routes[2].target:
  unknown target "azure-west/gpt-4o"
  did you mean "azure-eastus/gpt-4o-mini"?
```

---

## 九、与 LiteLLM 对比

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

## 十、Crate 选型
| 用途 | Crate | 说明 |
|------|-------|------|
| HTTP Server | `axum` + `tower` | Tower 中间件生态，流式友好 |
| HTTP Client | `hyper` + `hyper-util` | 底层零拷贝，连接池 |
| Async Runtime | `tokio` | 多线程运行时 |
| 序列化 | `serde` + `serde_json` + `serde_yaml` | 配置/请求解析 |
| etcd 客户端 | `etcd-client` | etcd v3 客户端, 支持 watch/lease/TXN |
| PostgreSQL | `sqlx` | 编译时 SQL 检查, 异步 |
| Redis | `redis` | 异步 + Lua 脚本 |
| JWT | `jsonwebtoken` | JWT 认证 |
| Key 哈希 | `argon2` | Virtual Key 安全存储 |
| 限流 | `governor` | Token Bucket / Leaky Bucket |
| 指标 | `prometheus` | Prometheus exporter |
| 追踪 | `tracing` + `opentelemetry` | 结构化日志 + OTEL |
| 配置原子切换 | `arc-swap` | ArcSwap 无锁读取 |
| UUID | `uuid` | request_id |
| 验证 | `validator` | 配置字段验证 |
| 字节处理 | `bytes` | 零拷贝 Bytes, 热 body 路径 |

---

## 十一、实施阶段
### Phase 1 — MVP（核心数据面）
构建优先:
- `/v1/chat/completions` + `/v1/embeddings`
- OpenAI 兼容的请求/响应面
- Providers: OpenAI, Azure OpenAI, Anthropic
- Auth: Virtual Key + JWT + IP Filter
- 路由: simple-shuffle + least-busy
- 首字节前 Fallback
- 限流: RPM/TPM/并发（Redis）
- 缓存: 内存 + Redis
- 可观测: tracing + Prometheus + OTEL
- etcd 配置源 + 热加载
- 基础 Spend 计费 + 响应头（cost/provider/cache hit）

**预计工期：4-6 周**

### Phase 2 — 生产基线
构建优先:
- DP Manager（etcd mock）适配
- Budget 层级（Global→Team→Member→Key→Customer）
- Per-Model 限流
- 健康检查 + Cooldown
- Latency-based / Usage-based 路由
- Callback sinks（Langfuse, Datadog）
- Request Mutation / Prompt Template
- 响应头增强（cost/provider/cache hit/remaining）

**预计工期：3-4 周**

### Phase 3 — 企业级
构建优先:
- Guardrail HTTP callback 引擎（pre/during/post-call）
- 密钥后端: AWS KMS / Vault / Azure Key Vault
- 语义缓存（Qdrant）
- 更多 Provider 适配器
- Admin 只读检查端点
- 告警（Slack/Email）
- Responses API / Images / Audio

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

## 参考项目
| 项目 | 说明 |
|------|------|
| LiteLLM | Python 开源 OpenAI 兼容 AI Gateway |
| Helicone/ai-gateway | Rust 生产的 AI Gateway |
| Grob | Rust LLM 路由代理 + DLP + 多 provider failover |
| ModelMux | Rust Vertex AI-OpenAI 代理 |
| CloakPipe | Rust PII 隐私代理（axum） |
| Apache APISIX | 高性能 API Gateway（参考其架构模式） |

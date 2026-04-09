---
name: arch-data-model
description: AISIX 数据模型约定、etcd 实体 schema 和配置编译规则
trigger:
  files:
    - "aisix/crates/aisix-config/**"
    - "aisix/crates/aisix-types/src/entities.rs"
    - "aisix/crates/aisix-types/src/request.rs"
    - "aisix/crates/aisix-types/src/usage.rs"
    - "aisix/crates/aisix-types/src/stream.rs"
    - "aisix/crates/aisix-core/src/context.rs"
    - "aisix/crates/aisix-policy/**"
  keywords:
    - "etcd"
    - "entity"
    - "snapshot"
    - "compiled snapshot"
    - "data model"
    - "rate limit"
    - "policy"
    - "provider config"
    - "model config"
    - "api key"
    - "request context"
    - "key meta"
    - "usage"
    - "transport mode"
priority: high
related:
  - arch-style
  - arch-api-design
  - arch-infra
---

# AISIX 数据模型与配置系统

## 核心架构原则

**不可变编译快照 + ArcSwap 原子切换。** 配置热加载零停机。
watcher 从 etcd 编译新快照后原子替换：依赖无效的当前资源会被跳过并视为 absent，硬错误则阻止发布并保留旧快照。

```
etcd watch → 后台 tokio 任务 → compile_snapshot() → ArcSwap.store()
```

## etcd Key 布局

所有运行时配置位于可配置前缀下（默认 `/aisix`）：

```
/aisix/providers/{provider_id}     # Provider 绑定信息
/aisix/models/{model_id}           # LLM 模型定义
/aisix/apikeys/{apikey_id}         # 客户端身份
/aisix/policies/{policy_id}        # 可复用的限流策略模板
```

## 实体 Schema

### Policy（可复用限流模板）

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

### Provider

```json
{
  "id": "openai-us",
  "kind": "openai",
  "base_url": "https://api.openai.com",
  "auth": { "secret_ref": "env:OPENAI_API_KEY" },
  "policy_id": "standard-tier"
}
```

`kind` 决定使用哪个 `ProviderCodec` 实现：
- `openai` → `OpenAICompatCodec`（覆盖 OpenAI、Azure、Ollama、vLLM、Groq）
- `anthropic`、`vertex`、`bedrock` → 独立 codec 实现

### Model

```json
{
  "id": "gpt-4o-mini",
  "provider_id": "openai-us",
  "upstream_model": "gpt-4.1-mini",
  "policy_id": "standard-tier"
}
```

### API Key

```json
{
  "id": "key-abc123",
  "key": "my-secret-key",
  "allowed_models": ["gpt-4o-mini", "claude-sonnet"],
  "policy_id": "standard-tier",
  "rate_limit": { "rpm": 100, "tpm": 50000 }
}
```

## 限流解析

### 三层独立检查

每个资源（provider、model、apikey）都可以携带自己的 `rate_limit`。
三层**独立检查**，执行顺序：provider → model → apikey。
任何一层超限立即返回 429。

### 内联覆盖语义

- 资源上的内联 `rate_limit` **覆盖** `policy_id` 引用
- 仅设置 `policy_id` 时，使用策略中的限流值
- 两者都未设置时，该层**不限流**（no-op）

### Redis Key 模式

| 维度 | Key 模式 |
|------|---------|
| TPM | `rl:tpm:{scope}:{id}:{model}:{window}` |
| RPM | `rl:rpm:{scope}:{id}:{model}:{window}` |
| 并发 | `rl:cc:{scope}:{id}:{model}`（Sorted Set） |
| 冷却 | `cooldown:{target_id}` |

## 核心 Rust 类型

### CanonicalRequest

统一内部请求类型，归一化所有入站 API 端点：

```rust
pub enum CanonicalRequest {
    Chat(ChatRequest),
    Embeddings(EmbeddingsRequest),
}
```

枚举上的方法（`model_name()`、`transport_mode()`）提供统一访问接口，
不感知具体是哪个 API 端点。

### TransportMode

决定响应转发路径。从请求计算得出，非存储字段：

```rust
pub enum TransportMode { SseStream, Json, BinaryStream }
```

- `SseStream`：Chat/Responses + `stream: true` → SSE 帧转发
- `Json`：Embeddings、Images、非流式 Chat/Responses → 完整 JSON 响应
- `BinaryStream`：AudioSpeech → 原始字节透传

### StreamEvent

最小化流式状态机：

```rust
pub enum StreamEvent {
    Delta(Bytes),     // 内容块
    Usage(Usage),     // 最终 token 计数
    Done,             // 流终止信号
}
```

所有上游格式（OpenAI SSE、Anthropic event/data、Gemini JSON lines、
Bedrock 二进制 eventstream）由 provider codec 统一解析为 `StreamEvent`。

### KeyMeta

认证 key 的身份信息结构体。不携带配置：

```rust
pub struct KeyMeta {
    pub key_id: String,
    pub user_id: Option<String>,
    pub customer_id: Option<String>,
    pub alias: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}
```

Pipeline 阶段用 `key_id` 从快照中查找各自需要的配置。
这保持了 KeyMeta 的小体积，避免认证与具体功能耦合。

### RequestContext

随 pipeline 推进逐步填充：

```rust
struct RequestContext {
    request: CanonicalRequest,    // Decode 后填入
    snapshot: Arc<CompiledSnapshot>,
    key_meta: KeyMeta,            // Authentication 后填入
    // resolved_route、usage 等由后续阶段填充
}
```

### CompiledSnapshot（`aisix-config`）

由 `compile_snapshot()` 产生的不可变、已验证的运行时配置：

```rust
pub struct CompiledSnapshot {
    pub revision: i64,
    pub keys_by_token: HashMap<String, KeyMeta>,
    pub apikeys_by_id: HashMap<String, ApiKeyConfig>,
    pub providers_by_id: HashMap<String, ProviderConfig>,
    pub models_by_name: HashMap<String, ModelConfig>,
    pub policies_by_id: HashMap<String, PolicyConfig>,
}
```

所有 HashMap 字段保证热路径 O(1) 查找。

## 配置编译规则

`compile_snapshot()` 是**纯函数**（无 I/O），返回 `Result<SnapshotCompileReport, String>`。

它执行以下规则：

1. 重复 ID 检测 → 两个实体共享同一 `id` 则拒绝
2. 重复明文 API key 检测 → 两个 key 共享同一 `key` 则拒绝
3. 外键/策略引用校验 → 引用缺失时跳过当前资源并记录 `CompileIssue`
4. 限流解析 → 合并策略默认值与内联覆盖

资源级语义：

- provider 引用缺失 policy → 该 provider 跳过
- model 引用缺失 provider 或 policy → 该 model 跳过
- apikey 引用缺失 model 或 policy → 该 apikey 跳过
- 被跳过资源在当前运行时快照中视为 absent，不保留旧版本

发布语义：

- 仅存在 `CompileIssue` 时，watcher 发布有效资源子集并记录日志
- 出现硬错误（如 duplicate id / duplicate token）时，本次发布失败，旧快照继续生效

## 四层存储模型

| 层级 | 位置 | 内容 |
|------|------|------|
| L1 热路径 | 进程内 `Arc<CompiledSnapshot>` | 路由索引、策略表、Provider 注册表 |
| L2 分布式计数器 | Redis | RPM/TPM 计数器、并发租约、冷却标记 |
| L3 共享缓存 | Redis / 进程内内存 | 响应缓存（Phase 1: moka/DashMap 进程内） |
| L4 持久化真相 | PostgreSQL（仅控制面） | 用量账本、审计日志、预算定义 |

## Serde 约定

- 所有配置结构体双向可序列化（`Serialize + Deserialize`）
- `#[serde(rename = "...")]` 用于线上格式的枚举变体名：`#[serde(rename = "openai")] OpenAi`
- `#[serde(default)]` 用于向后兼容的可选字段
- `#[serde(rename = "type")]` 用于 Rust 保留字命名的字段

## 当前阶段妥协

- [!NOTE] API Key 以明文存储在 etcd 中。安全边界依赖 etcd 访问控制 + TLS。
  Phase 3 将迁移到 BLAKE3/SHA256 哈希存储，并集成 KMS/Vault。
- [!NOTE] Secret 解析仅支持 `env:` 前缀（`env:OPENAI_API_KEY`）。
  Phase 3 新增 AWS KMS、Vault、Azure Key Vault 后端。
- [!NOTE] Phase 1 缓存仅限进程内内存（moka/DashMap，LRU，上限 10000 条，TTL 300s）。
  Phase 2 新增 Redis 共享缓存；Phase 3 新增语义缓存（Qdrant）。

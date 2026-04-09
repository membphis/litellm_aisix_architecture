# 核心 Rust 类型

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

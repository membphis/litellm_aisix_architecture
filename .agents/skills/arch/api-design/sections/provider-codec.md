# Provider Codec 接口

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

- 客户端 ingress 协议与上游 provider kind 解耦。`/v1/messages` 可以走 Anthropic 客户端协议，但继续复用 `OpenAiCompatCodec` 访问 OpenAI-compatible 上游。
- `OpenAICompatCodec`：通用实现，覆盖所有 OpenAI 兼容 Provider
  （OpenAI、Azure、Ollama、vLLM、Groq）— 通过 base_url + auth 策略参数化
- 非兼容 Provider（Anthropic、Vertex、Bedrock）：各自独立实现
- 统一以 `Arc<dyn ProviderCodec>` 持有在 `ProviderRegistry` 中

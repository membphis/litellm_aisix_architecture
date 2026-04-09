# StreamChunk（核心复杂度）

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

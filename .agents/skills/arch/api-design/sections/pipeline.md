# 固定 Pipeline 序列

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

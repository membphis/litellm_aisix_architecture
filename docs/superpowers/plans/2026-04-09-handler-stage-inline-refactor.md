# Handler Stage Inline Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `chat_completions` and `embeddings` so their request paths read as direct ordered stage lists while preserving all existing behavior.

**Architecture:** Move orchestration responsibility out of `crates/aisix-server/src/pipeline.rs` into stage-oriented modules under `crates/aisix-server/src/pipeline/`, with handlers calling those stages directly in order. Keep functional behavior unchanged by wrapping existing logic, reshaping `RequestContext` for staged population, and preserving the current response-building and usage-recording semantics.

**Tech Stack:** Rust, Axum, Tokio, workspace crates (`aisix-server`, `aisix-core`, `aisix-policy`, `aisix-router`, `aisix-ratelimit`, `aisix-cache`, `aisix-spend`)

---

## File Map

### Existing files to modify

- `aisix/crates/aisix-core/src/context.rs`
  - Reshape `RequestContext` from a fully-populated struct into a staged-population context used by handlers and pipeline stages.
- `aisix/crates/aisix-core/src/lib.rs`
  - Keep re-export aligned if `RequestContext` constructor/helpers are added.
- `aisix/crates/aisix-server/src/lib.rs`
  - Keep module exports aligned after converting `pipeline.rs` into a directory module.
- `aisix/crates/aisix-server/src/handlers/chat.rs`
  - Replace `run_json_pipeline` / `run_chat_stream_pipeline` calls with direct stage sequencing.
- `aisix/crates/aisix-server/src/handlers/embeddings.rs`
  - Replace `run_json_pipeline` call with direct stage sequencing.
- `aisix/crates/aisix-server/src/pipeline.rs`
  - Replace with `pipeline/mod.rs` and split logic into stage modules.
- `aisix/crates/aisix-server/tests/chat_non_stream.rs`
  - Update helper imports if `build_response` moves to a stage module.
- `aisix/crates/aisix-server/tests/stream_chat.rs`
  - Update helper imports if stream response helpers move.
- `aisix/crates/aisix-server/tests/embeddings.rs`
  - Rename assertions if the test names mention `run_json_pipeline` directly.

### New files to create

- `aisix/crates/aisix-server/src/pipeline/mod.rs`
  - Stage module declarations and public re-exports used by handlers/tests.
- `aisix/crates/aisix-server/src/pipeline/authorization.rs`
  - Wrap `ensure_model_allowed` into stage-shaped API.
- `aisix/crates/aisix-server/src/pipeline/rate_limit.rs`
  - Wrap `RateLimitService::precheck` into a stage API that uses context and state.
- `aisix/crates/aisix-server/src/pipeline/cache.rs`
  - Chat cache lookup and writeback helpers.
- `aisix/crates/aisix-server/src/pipeline/route_select.rs`
  - Resolve and store `ResolvedTarget` and selected provider information on context.
- `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
  - Non-stream and stream upstream execution plus response rebuilding.
- `aisix/crates/aisix-server/src/pipeline/post_call.rs`
  - Usage-recording stage helpers.

### Existing files to reuse without planned modification

- `aisix/crates/aisix-auth/src/extractor.rs`
  - Authentication stays as-is.
- `aisix/crates/aisix-server/src/stream_proxy.rs`
  - Stream response rebuilding helper remains reusable from the new `stream_chunk` stage.
- `aisix/crates/aisix-policy/src/access.rs`
  - Existing authorization logic remains authoritative.
- `aisix/crates/aisix-router/src/resolve.rs`
  - Existing route resolution logic remains authoritative.
- `aisix/crates/aisix-ratelimit/src/service.rs`
  - Existing limiter behavior remains authoritative.
- `aisix/crates/aisix-spend/src/recorder.rs`
  - Existing usage recorder remains authoritative.

### Test commands to use during execution

- `cargo test -p aisix-server auth_flow -- --nocapture`
- `cargo test -p aisix-server rate_limit -- --nocapture`
- `cargo test -p aisix-server chat_non_stream -- --nocapture`
- `cargo test -p aisix-server stream_chat -- --nocapture`
- `cargo test -p aisix-server embeddings -- --nocapture`
- `cargo test -p aisix-server --tests`

## Task 1: Reshape RequestContext For Staged Population

**Files:**
- Modify: `aisix/crates/aisix-core/src/context.rs`
- Modify: `aisix/crates/aisix-core/src/lib.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Add a focused context unit test in `context.rs`**

```rust
#[cfg(test)]
mod tests {
    use aisix_types::{
        entities::KeyMeta,
        request::{CanonicalRequest, EmbeddingsRequest},
    };

    use super::RequestContext;

    #[test]
    fn new_context_starts_without_resolved_route_or_usage() {
        let ctx = RequestContext::new(
            CanonicalRequest::Embeddings(EmbeddingsRequest {
                model: "text-embedding-3-small".to_string(),
                input: serde_json::json!("hello"),
            }),
            KeyMeta {
                key_id: "vk_123".to_string(),
                user_id: None,
                customer_id: None,
                alias: Some("test-key".to_string()),
                expires_at: None,
                allowed_models: vec!["text-embedding-3-small".to_string()],
            },
        );

        assert!(ctx.resolved_target.is_none());
        assert!(ctx.resolved_provider_id.is_none());
        assert!(ctx.usage.is_none());
        assert!(!ctx.response_cached);
    }
}
```

- [ ] **Step 2: Run the context test to verify it fails**

Run: `cargo test -p aisix-core new_context_starts_without_resolved_route_or_usage -- --nocapture`
Expected: FAIL because `RequestContext::new`, `resolved_target`, `resolved_provider_id`, `Option<Usage>`, or `response_cached` do not exist yet.

- [ ] **Step 3: Rewrite `RequestContext` with staged fields and a constructor**

Replace `aisix/crates/aisix-core/src/context.rs` with code shaped like this:

```rust
use uuid::Uuid;

use aisix_router::resolve::ResolvedTarget;
use aisix_types::{entities::KeyMeta, request::CanonicalRequest, usage::Usage};

#[derive(Debug, Clone, PartialEq)]
pub struct RequestContext {
    pub request_id: Uuid,
    pub request: CanonicalRequest,
    pub key_meta: KeyMeta,
    pub resolved_target: Option<ResolvedTarget>,
    pub resolved_provider_id: Option<String>,
    pub usage: Option<Usage>,
    pub response_cached: bool,
}

impl RequestContext {
    pub fn new(request: CanonicalRequest, key_meta: KeyMeta) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            request,
            key_meta,
            resolved_target: None,
            resolved_provider_id: None,
            usage: None,
            response_cached: false,
        }
    }
}
```

If `aisix-core` does not currently depend on `aisix-router`, add the dependency in `aisix/crates/aisix-core/Cargo.toml`:

```toml
aisix-router = { path = "../aisix-router" }
serde_json.workspace = true
```

- [ ] **Step 4: Run the context test to verify it passes**

Run: `cargo test -p aisix-core new_context_starts_without_resolved_route_or_usage -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-core/Cargo.toml aisix/crates/aisix-core/src/context.rs aisix/crates/aisix-core/src/lib.rs
git commit -m "refactor: stage request context population"
```

## Task 2: Create Stage-Oriented Pipeline Module Skeleton

**Files:**
- Create: `aisix/crates/aisix-server/src/pipeline/mod.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/authorization.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/rate_limit.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/cache.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/route_select.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
- Create: `aisix/crates/aisix-server/src/pipeline/post_call.rs`
- Modify: `aisix/crates/aisix-server/src/lib.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Add a failing smoke test that compiles against the new module API**

Append this test near the response-helper tests in `aisix/crates/aisix-server/tests/chat_non_stream.rs`:

```rust
#[test]
fn pipeline_stage_modules_export_response_helpers() {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let response = aisix_server::pipeline::stream_chunk::build_json_response(
        StatusCode::OK,
        Body::from("{\"ok\":true}"),
        headers,
        Some("false"),
        Some("openai"),
        None,
    )
    .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run the new test to verify it fails**

Run: `cargo test -p aisix-server pipeline_stage_modules_export_response_helpers -- --nocapture`
Expected: FAIL because `pipeline::stream_chunk::build_json_response` and the new module tree do not exist yet.

- [ ] **Step 3: Replace `pipeline.rs` with the new module tree and stubs**

Delete `aisix/crates/aisix-server/src/pipeline.rs`, create `aisix/crates/aisix-server/src/pipeline/mod.rs`, and add stage files with compiling stubs. Use this shape:

```rust
// aisix/crates/aisix-server/src/pipeline/mod.rs
pub mod authorization;
pub mod cache;
pub mod post_call;
pub mod rate_limit;
pub mod route_select;
pub mod stream_chunk;
```

```rust
// aisix/crates/aisix-server/src/pipeline/authorization.rs
use aisix_core::RequestContext;
use aisix_types::error::GatewayError;

pub fn check(_ctx: &RequestContext) -> Result<(), GatewayError> {
    todo!()
}
```

```rust
// aisix/crates/aisix-server/src/pipeline/rate_limit.rs
use aisix_core::RequestContext;
use aisix_types::error::GatewayError;

use crate::app::ServerState;

pub async fn check(_ctx: &RequestContext, _state: &ServerState) -> Result<(), GatewayError> {
    todo!()
}
```

```rust
// aisix/crates/aisix-server/src/pipeline/cache.rs
use aisix_core::RequestContext;
use aisix_types::error::GatewayError;
use axum::{body::Body, http::Response};

use crate::app::ServerState;

pub async fn lookup_chat(
    _ctx: &mut RequestContext,
    _state: &ServerState,
) -> Result<Option<Response<Body>>, GatewayError> {
    todo!()
}
```

```rust
// aisix/crates/aisix-server/src/pipeline/route_select.rs
use aisix_core::RequestContext;
use aisix_types::error::GatewayError;

use crate::app::ServerState;

pub fn resolve(_ctx: &mut RequestContext, _state: &ServerState) -> Result<(), GatewayError> {
    todo!()
}
```

```rust
// aisix/crates/aisix-server/src/pipeline/post_call.rs
use aisix_core::RequestContext;

use crate::app::ServerState;

pub async fn record_success(_ctx: &RequestContext, _state: &ServerState) {}
```

```rust
// aisix/crates/aisix-server/src/pipeline/stream_chunk.rs
use aisix_types::error::{ErrorKind, GatewayError};
use axum::{
    body::Body,
    http::{header, HeaderName, HeaderValue, Response},
};

pub async fn proxy(
    _ctx: &mut aisix_core::RequestContext,
    _state: &crate::app::ServerState,
) -> Result<Response<Body>, GatewayError> {
    Err(GatewayError {
        kind: ErrorKind::Internal,
        message: "stage proxy not implemented".to_string(),
    })
}

pub fn build_json_response(
    status: http::StatusCode,
    body: impl Into<Body>,
    mut headers: http::HeaderMap,
    cache_hit: Option<&str>,
    provider_id: Option<&str>,
    usage: Option<aisix_types::usage::Usage>,
) -> Result<Response<Body>, GatewayError> {
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::CONNECTION);

    if cache_hit.is_some() && !headers.contains_key(http::header::CONTENT_TYPE) {
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }

    let mut response = Response::builder()
        .status(status)
        .body(body.into())
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build response: {error}"),
        })?;

    response.headers_mut().extend(headers);

    if let Some(cache_hit) = cache_hit {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-cache-hit"),
            HeaderValue::from_str(cache_hit).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set cache header: {error}"),
            })?,
        );
    }

    if let Some(provider_id) = provider_id {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-provider"),
            HeaderValue::from_str(provider_id).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set provider header: {error}"),
            })?,
        );
    }

    if let Some(usage) = usage {
        response.extensions_mut().insert(usage);
    }

    Ok(response)
}
```

Ensure `aisix/crates/aisix-server/src/lib.rs` continues to expose `pub mod pipeline;` after the file-to-directory module conversion.

- [ ] **Step 4: Run the module smoke test to verify it passes**

Run: `cargo test -p aisix-server pipeline_stage_modules_export_response_helpers -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/lib.rs aisix/crates/aisix-server/src/pipeline aisix/crates/aisix-server/tests/chat_non_stream.rs
git commit -m "refactor: split pipeline into stage modules"
```

## Task 3: Move Authorization And Route Selection Into Stages

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/authorization.rs`
- Modify: `aisix/crates/aisix-server/src/pipeline/route_select.rs`
- Test: `aisix/crates/aisix-server/tests/auth_flow.rs`

- [ ] **Step 1: Add a failing integration assertion that the direct stage path still rejects disallowed models**

No new test file is needed. Use the existing `disallowed_model_returns_403` test in `aisix/crates/aisix-server/tests/auth_flow.rs` as the failing guard for this task.

- [ ] **Step 2: Run the auth-flow test to verify the current stage stubs fail**

Run: `cargo test -p aisix-server disallowed_model_returns_403 -- --nocapture`
Expected: FAIL because the stage stubs are still `todo!()`.

- [ ] **Step 3: Implement `authorization::check` and `route_select::resolve`**

Use this code shape:

```rust
// authorization.rs
use aisix_core::RequestContext;
use aisix_policy::access::ensure_model_allowed;
use aisix_types::error::GatewayError;

pub fn check(ctx: &RequestContext) -> Result<(), GatewayError> {
    ensure_model_allowed(&ctx.key_meta, ctx.request.model_name())
}
```

```rust
// route_select.rs
use aisix_core::RequestContext;
use aisix_router::resolve::resolve_fixed_model;
use aisix_types::{
    error::{ErrorKind, GatewayError},
};

use crate::app::ServerState;

pub fn resolve(ctx: &mut RequestContext, state: &ServerState) -> Result<(), GatewayError> {
    let snapshot = state.app.snapshot.load();
    let target = resolve_fixed_model(&snapshot, ctx.request.model_name())?;
    let provider = snapshot
        .providers_by_id
        .get(&target.provider_id)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("provider '{}' not found", target.provider_id),
        })?;

    ctx.resolved_provider_id = Some(provider.id.clone());
    ctx.resolved_target = Some(target);
    Ok(())
}
```

- [ ] **Step 4: Run the auth-flow test to verify it passes again**

Run: `cargo test -p aisix-server auth_flow -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/pipeline/authorization.rs aisix/crates/aisix-server/src/pipeline/route_select.rs
git commit -m "refactor: move auth and route selection into stages"
```

## Task 4: Move Rate Limit Checking Into A Stage

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/rate_limit.rs`
- Test: `aisix/crates/aisix-server/tests/rate_limit.rs`

- [ ] **Step 1: Use the existing rate-limit regression test as the failing guard**

Use `inline_rpm_limit_triggers_429` in `aisix/crates/aisix-server/tests/rate_limit.rs`.

- [ ] **Step 2: Run the rate-limit test to verify the stage stub fails**

Run: `cargo test -p aisix-server inline_rpm_limit_triggers_429 -- --nocapture`
Expected: FAIL because `rate_limit::check` is still `todo!()`.

- [ ] **Step 3: Implement `rate_limit::check` using context and resolved route data**

Use this code shape:

```rust
use aisix_core::RequestContext;
use aisix_types::{
    error::{ErrorKind, GatewayError},
};

use crate::app::ServerState;

pub async fn check(ctx: &RequestContext, state: &ServerState) -> Result<(), GatewayError> {
    let provider_id = ctx.resolved_provider_id.as_deref().ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: "resolved provider id missing before rate-limit check".to_string(),
    })?;

    let snapshot = state.app.snapshot.load();
    let _guard = state
        .app
        .rate_limits
        .precheck(
            &snapshot,
            &ctx.key_meta.key_id,
            ctx.request.model_name(),
            provider_id,
        )
        .await?;

    Ok(())
}
```

Then update the handler plan later so `route_select::resolve` runs before `rate_limit::check`. This is acceptable because the current implementation already resolves route before precheck. The visible handler ordering should reflect the true data dependency rather than an aspirational order that cannot yet compile.

- [ ] **Step 4: Run the rate-limit suite to verify it passes**

Run: `cargo test -p aisix-server rate_limit -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/pipeline/rate_limit.rs
git commit -m "refactor: move rate limit precheck into stage"
```

## Task 5: Move Chat Cache Logic Into A Stage

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/cache.rs`
- Modify: `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Use the existing chat cache regression test as the failing guard**

Use `repeated_non_stream_chat_hits_memory_cache` in `aisix/crates/aisix-server/tests/chat_non_stream.rs`.

- [ ] **Step 2: Run the cache regression test to verify the stage stub fails**

Run: `cargo test -p aisix-server repeated_non_stream_chat_hits_memory_cache -- --nocapture`
Expected: FAIL because `cache::lookup_chat` is still `todo!()`.

- [ ] **Step 3: Implement cache lookup and writeback helpers**

Use this code shape:

```rust
// cache.rs
use aisix_cache::{CachedChatResponse, build_chat_cache_key};
use aisix_core::RequestContext;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
    usage::Usage,
};
use axum::{body::Body, http::Response};

use crate::{app::ServerState, pipeline::stream_chunk::build_json_response};

pub async fn lookup_chat(
    ctx: &mut RequestContext,
    state: &ServerState,
) -> Result<Option<Response<Body>>, GatewayError> {
    let CanonicalRequest::Chat(chat_request) = &ctx.request else {
        return Ok(None);
    };

    let target = ctx.resolved_target.as_ref().ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: "resolved target missing before cache lookup".to_string(),
    })?;
    let provider_id = ctx.resolved_provider_id.as_deref().ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: "resolved provider id missing before cache lookup".to_string(),
    })?;

    let snapshot = state.app.snapshot.load();
    let cache_key = build_chat_cache_key(
        snapshot.revision,
        provider_id,
        &target.upstream_model,
        &chat_request.model,
        &chat_request.messages,
    )
    .map_err(|error| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("failed to build chat cache key: {error}"),
    })?;

    let Some(cached) = state.app.cache.get_chat(&cache_key) else {
        return Ok(None);
    };

    ctx.response_cached = true;
    ctx.usage = cached.usage.clone();

    let response = build_json_response(
        http::StatusCode::OK,
        cached.body,
        http::HeaderMap::new(),
        Some("true"),
        Some(&cached.provider_id),
        cached.usage,
    )?;

    Ok(Some(response))
}

pub fn store_chat_success(
    ctx: &RequestContext,
    state: &ServerState,
    response_body: &[u8],
    usage: Option<Usage>,
) -> Result<(), GatewayError> {
    let CanonicalRequest::Chat(chat_request) = &ctx.request else {
        return Ok(());
    };

    if ctx.request.transport_mode() != aisix_types::usage::TransportMode::Json {
        return Ok(());
    }

    let target = ctx.resolved_target.as_ref().ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: "resolved target missing before cache store".to_string(),
    })?;
    let provider_id = ctx.resolved_provider_id.as_deref().ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: "resolved provider id missing before cache store".to_string(),
    })?;

    let snapshot = state.app.snapshot.load();
    let cache_key = build_chat_cache_key(
        snapshot.revision,
        provider_id,
        &target.upstream_model,
        &chat_request.model,
        &chat_request.messages,
    )
    .map_err(|error| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("failed to build chat cache key: {error}"),
    })?;

    state.app.cache.put_chat(
        cache_key,
        CachedChatResponse {
            body: response_body.to_vec(),
            provider_id: provider_id.to_string(),
            usage,
        },
    );

    Ok(())
}
```

- [ ] **Step 4: Run the chat cache regression test to verify it passes**

Run: `cargo test -p aisix-server chat_non_stream -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/pipeline/cache.rs aisix/crates/aisix-server/src/pipeline/stream_chunk.rs
git commit -m "refactor: extract chat cache stage"
```

## Task 6: Move Upstream Execution And Response Rebuilding Into `stream_chunk`

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`
- Test: `aisix/crates/aisix-server/tests/embeddings.rs`

- [ ] **Step 1: Use the existing transport tests as failing guards**

Use:

- `chat_non_stream_proxies_openai_compatible_json`
- `stream_chat_returns_valid_openai_sse`
- `embeddings_reuse_the_json_pipeline`

- [ ] **Step 2: Run the transport tests to verify the stage stub fails**

Run: `cargo test -p aisix-server chat_non_stream_proxies_openai_compatible_json -- --nocapture && cargo test -p aisix-server stream_chat_returns_valid_openai_sse -- --nocapture && cargo test -p aisix-server embeddings_reuse_the_json_pipeline -- --nocapture`
Expected: FAIL because `stream_chunk::proxy` is still a stub.

- [ ] **Step 3: Implement `stream_chunk::proxy` by moving existing pipeline logic**

Move the execution logic out of old `pipeline.rs` into `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`.

Use these function boundaries:

```rust
pub async fn proxy(
    ctx: &mut RequestContext,
    state: &ServerState,
) -> Result<Response<Body>, GatewayError>
```

Inside `proxy`:

- Load `ctx.resolved_target` and `ctx.resolved_provider_id`
- Re-load the current snapshot to fetch the provider config
- Resolve the provider codec with `state.providers.resolve(provider)?`
- Branch on `ctx.request.transport_mode()`
  - For `TransportMode::Json`
    - call `codec.execute_json(provider, &target.upstream_model, &ctx.request).await?`
    - clone `output.usage` into `ctx.usage`
    - if `output.status.is_success()`, call `cache::store_chat_success(ctx, state, &output.body, output.usage.clone())?`
    - return `build_json_response(output.status, output.body, output.headers, cache_header, Some(&provider.id), output.usage)` where `cache_header` is `Some("false")` only for `CanonicalRequest::Chat`, otherwise `None`
  - For `TransportMode::SseStream`
    - call `codec.execute_stream(provider, &target.upstream_model, &ctx.request).await?`
    - clone `output.usage` into `ctx.usage`
    - return `crate::stream_proxy::build_stream_response(output.status, output.body, output.headers, &provider.id, output.usage)`

Preserve the existing error text for missing provider config:

```rust
GatewayError {
    kind: ErrorKind::Internal,
    message: format!("provider '{}' not found", provider_id),
}
```

Also move `build_response` from the old file into `build_json_response` and update `chat_non_stream.rs` to use the new path:

```rust
let response = aisix_server::pipeline::stream_chunk::build_json_response(...)
```

- [ ] **Step 4: Run the transport suites to verify they pass**

Run: `cargo test -p aisix-server chat_non_stream -- --nocapture && cargo test -p aisix-server stream_chat -- --nocapture && cargo test -p aisix-server embeddings -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/pipeline/stream_chunk.rs aisix/crates/aisix-server/tests/chat_non_stream.rs aisix/crates/aisix-server/tests/stream_chat.rs aisix/crates/aisix-server/tests/embeddings.rs
git commit -m "refactor: move upstream execution into stream chunk stage"
```

## Task 7: Move Usage Recording Into `post_call`

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/post_call.rs`
- Test: `aisix/crates/aisix-server/tests/rate_limit.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Use existing usage-regression tests as failing guards**

Use:

- `failed_upstream_does_not_record_usage`
- `stream_chat_records_and_exposes_usage_when_upstream_includes_it`
- `repeated_non_stream_chat_hits_memory_cache`

- [ ] **Step 2: Run the usage tests to verify the stage stub fails**

Run: `cargo test -p aisix-server failed_upstream_does_not_record_usage -- --nocapture && cargo test -p aisix-server stream_chat_records_and_exposes_usage_when_upstream_includes_it -- --nocapture && cargo test -p aisix-server repeated_non_stream_chat_hits_memory_cache -- --nocapture`
Expected: FAIL because `post_call::record_success` is still a no-op.

- [ ] **Step 3: Implement `post_call::record_success` using context data**

Use this code shape:

```rust
use aisix_core::RequestContext;

use crate::app::ServerState;

pub async fn record_success(ctx: &RequestContext, state: &ServerState) {
    let Some(usage) = ctx.usage.as_ref() else {
        return;
    };

    state
        .app
        .usage_recorder
        .record_success(&ctx.key_meta, ctx.request.model_name(), usage)
        .await;
}
```

- [ ] **Step 4: Run the usage tests to verify they pass**

Run: `cargo test -p aisix-server rate_limit -- --nocapture && cargo test -p aisix-server stream_chat -- --nocapture && cargo test -p aisix-server chat_non_stream -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/pipeline/post_call.rs
git commit -m "refactor: move usage recording into post-call stage"
```

## Task 8: Rewrite Handlers To Show Direct Stage Ordering

**Files:**
- Modify: `aisix/crates/aisix-server/src/handlers/chat.rs`
- Modify: `aisix/crates/aisix-server/src/handlers/embeddings.rs`
- Test: `aisix/crates/aisix-server/tests/auth_flow.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`
- Test: `aisix/crates/aisix-server/tests/embeddings.rs`

- [ ] **Step 1: Use existing end-to-end handler tests as failing guards**

Use the existing suites:

- `auth_flow`
- `chat_non_stream`
- `stream_chat`
- `embeddings`

- [ ] **Step 2: Run a chat-focused test to verify the old entrypoints still gate the rewrite**

Run: `cargo test -p aisix-server happy_path_auth_and_route_returns_200 -- --nocapture`
Expected: PASS before the rewrite, providing a baseline.

- [ ] **Step 3: Rewrite `chat_completions` and `embeddings` to call stages directly**

Replace handler bodies with code shaped like this:

```rust
// chat.rs
use aisix_auth::extractor::AuthenticatedKey;
use aisix_core::RequestContext;
use aisix_types::{
    error::GatewayError,
    request::{CanonicalRequest, ChatRequest},
};
use axum::{extract::State, response::Response, Json};

use crate::{
    app::ServerState,
    pipeline::{authorization, cache, post_call, rate_limit, route_select, stream_chunk},
};

pub async fn chat_completions(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<ChatRequest>,
) -> Result<Response, GatewayError> {
    let mut ctx = RequestContext::new(CanonicalRequest::Chat(request), authenticated_key.meta);

    authorization::check(&ctx)?;
    route_select::resolve(&mut ctx, &state)?;
    rate_limit::check(&ctx, &state).await?;

    if let Some(response) = cache::lookup_chat(&mut ctx, &state).await? {
        post_call::record_success(&ctx, &state).await;
        return Ok(response);
    }

    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    post_call::record_success(&ctx, &state).await;
    Ok(response)
}
```

```rust
// embeddings.rs
use aisix_auth::extractor::AuthenticatedKey;
use aisix_core::RequestContext;
use aisix_types::{
    error::GatewayError,
    request::{CanonicalRequest, EmbeddingsRequest},
};
use axum::{extract::State, response::Response, Json};

use crate::{
    app::ServerState,
    pipeline::{authorization, post_call, rate_limit, route_select, stream_chunk},
};

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
    route_select::resolve(&mut ctx, &state)?;
    rate_limit::check(&ctx, &state).await?;
    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    post_call::record_success(&ctx, &state).await;
    Ok(response)
}
```

The visible order should stay compact and stage-oriented, even though `route_select` appears before `rate_limit` to satisfy the current provider-id dependency in the limiter implementation.

- [ ] **Step 4: Run the handler suites to verify they pass**

Run: `cargo test -p aisix-server auth_flow -- --nocapture && cargo test -p aisix-server chat_non_stream -- --nocapture && cargo test -p aisix-server stream_chat -- --nocapture && cargo test -p aisix-server embeddings -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/src/handlers/chat.rs aisix/crates/aisix-server/src/handlers/embeddings.rs
git commit -m "refactor: inline handler stage ordering"
```

## Task 9: Remove Old Orchestration Entry Points And Run Full Regression

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/mod.rs`
- Modify: `aisix/crates/aisix-server/tests/chat_non_stream.rs`
- Test: `aisix/crates/aisix-server/tests/auth_flow.rs`
- Test: `aisix/crates/aisix-server/tests/rate_limit.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`
- Test: `aisix/crates/aisix-server/tests/embeddings.rs`

- [ ] **Step 1: Search for stale references to removed orchestration functions**

Run: `rg "run_json_pipeline|run_chat_stream_pipeline|build_response\(" aisix/crates/aisix-server`
Expected: Only stage-module helpers and updated tests remain; no handler references to the old orchestration functions.

- [ ] **Step 2: Remove any dead exports or compatibility shims left from the old file**

Clean up `aisix/crates/aisix-server/src/pipeline/mod.rs` so it only exports the stage modules that are still used:

```rust
pub mod authorization;
pub mod cache;
pub mod post_call;
pub mod rate_limit;
pub mod route_select;
pub mod stream_chunk;
```

If any compatibility wrapper remains from the old `pipeline.rs`, delete it rather than preserving it.

- [ ] **Step 3: Run the full `aisix-server` test suite**

Run: `cargo test -p aisix-server --tests`
Expected: PASS

- [ ] **Step 4: Run workspace tests to catch spillover**

Run: `cargo test --manifest-path aisix/Cargo.toml --workspace`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-core/src/context.rs aisix/crates/aisix-server/src/handlers/chat.rs aisix/crates/aisix-server/src/handlers/embeddings.rs aisix/crates/aisix-server/src/lib.rs aisix/crates/aisix-server/src/pipeline aisix/crates/aisix-server/tests/chat_non_stream.rs aisix/crates/aisix-server/tests/embeddings.rs
git commit -m "refactor: complete handler stage inline refactor"
```

## Self-Review Notes

### Spec coverage

- Handler direct stage ordering: covered by Task 8.
- Stage module extraction: covered by Tasks 2 through 7.
- RequestContext staged population: covered by Task 1.
- No new shared generic `run_pipeline()`: enforced by Tasks 8 and 9.
- Behavior preservation for chat, streaming chat, embeddings, auth, rate limiting, cache, usage: covered by Tasks 3 through 9 with existing test suites.
- Ordering note: the confirmed design prefers a readable stage list; the implementation order in Task 8 places `route_select` before `rate_limit` because the current limiter requires a resolved provider id. This keeps the handler compact and stage-oriented while matching today's real dependency graph.

### Placeholder scan

- No `TBD`, `TODO`, or deferred implementation markers remain in the plan.
- Each code-changing step contains concrete file paths, function names, and code shapes.

### Type consistency

- `RequestContext` uses `resolved_target`, `resolved_provider_id`, `usage`, and `response_cached` consistently across all tasks.
- Handler calls consistently reference `authorization::check`, `route_select::resolve`, `rate_limit::check`, `cache::lookup_chat`, `stream_chunk::proxy`, and `post_call::record_success`.

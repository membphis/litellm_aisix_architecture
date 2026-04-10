# AISIX Phase 1 MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a working `aisix` AI Gateway MVP that serves `/v1/chat/completions` and `/v1/embeddings` with Virtual Key auth, fixed model routing, Redis rate limiting, memory chat cache, etcd hot reload, minimal Admin API, and a runnable local demo.

**Architecture:** Create the full Rust workspace shape up front, but implement it as vertical slices. Every slice must end in a runnable state: first non-streaming chat, then embeddings reuse, then Redis protection, then SSE streaming, then hot reload and Admin, then demo/docs polish. Request handling always reads from an immutable `CompiledSnapshot` behind `ArcSwap`; Admin writes only to etcd, and runtime changes arrive only through the watch/recompile path.

**Tech Stack:** Rust workspace, tokio, axum, tower, reqwest, etcd-client, redis, arc-swap, moka, governor, tracing, prometheus, opentelemetry, serde, wiremock

---

## Scope Guardrails

This plan implements only Phase 1 from `docs/superpowers/specs/2026-04-08-aisix-phase1-design.md`.

Explicitly out of scope:

- budget enforcement
- multi-deployment routing and fallback
- guardrails
- prompt mutation
- Redis/shared cache
- Responses API / Images / Audio / Realtime / MCP
- pricing / cost calculation
- standalone `aisix-admin`

Implementation rule: do not create speculative systems for out-of-scope features. Leave clean boundaries instead.

## File Structure Lock-In

All new code lives under `aisix/`.

```text
aisix/
├── Cargo.toml
├── docker-compose.yml
├── config/
│   └── aisix-gateway.example.yaml
├── scripts/
│   └── smoke-phase1.sh
├── bin/
│   └── aisix-gateway/
│       ├── Cargo.toml
│       └── src/main.rs
└── crates/
    ├── aisix-types/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── error.rs
    │       ├── request.rs
    │       ├── stream.rs
    │       ├── usage.rs
    │       └── entities.rs
    ├── aisix-core/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── context.rs
    │       └── app_state.rs
    ├── aisix-config/
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── startup.rs
    │   │   ├── etcd_model.rs
    │   │   ├── snapshot.rs
    │   │   ├── compile.rs
    │   │   └── watcher.rs
    │   └── tests/
    │       ├── startup_config.rs
    │       └── snapshot_compile.rs
    ├── aisix-storage/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── redis_pool.rs
    │       └── counters.rs
    ├── aisix-auth/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       └── extractor.rs
    ├── aisix-policy/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── access.rs
    │       └── limits.rs
    ├── aisix-router/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       └── resolve.rs
    ├── aisix-ratelimit/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── shadow.rs
    │       ├── redis_check.rs
    │       ├── concurrency.rs
    │       └── service.rs
    ├── aisix-cache/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── key.rs
    │       └── memory.rs
    ├── aisix-providers/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── codec.rs
    │       ├── registry.rs
    │       ├── openai_compat.rs
    │       ├── openai_sse.rs
    │       ├── anthropic.rs
    │       └── anthropic_sse.rs
    ├── aisix-spend/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── event.rs
    │       └── recorder.rs
    ├── aisix-observability/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── tracing_init.rs
    │       └── metrics.rs
    ├── aisix-runtime/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       └── bootstrap.rs
    └── aisix-server/
        ├── Cargo.toml
        ├── src/
        │   ├── lib.rs
        │   ├── app.rs
        │   ├── pipeline.rs
        │   ├── stream_proxy.rs
        │   ├── health.rs
        │   ├── handlers/
        │   │   ├── mod.rs
        │   │   ├── chat.rs
        │   │   └── embeddings.rs
        │   └── admin/
        │       ├── mod.rs
        │       ├── auth.rs
        │       ├── providers.rs
        │       ├── models.rs
        │       ├── apikeys.rs
        │       └── policies.rs
        └── tests/
            ├── auth_flow.rs
            ├── chat_non_stream.rs
            ├── embeddings.rs
            ├── rate_limit.rs
            ├── stream_chat.rs
            ├── hot_reload.rs
            └── admin_api.rs
```

## Vertical Slice Order

1. Workspace skeleton and dev environment
2. Shared types and immutable snapshot compilation
3. Runtime bootstrap and startup health
4. Auth + policy + fixed routing
5. OpenAI-compatible non-stream chat
6. Embeddings reuse through the same pipeline
7. Redis rate limiting and usage recording
8. Memory chat cache and response headers
9. OpenAI SSE streaming
10. Anthropic codec and SSE normalization
11. Admin API and etcd hot reload
12. Observability, smoke script, and getting started docs

---

### Task 1: Initialize the workspace root and local dev environment

**Files:**
- Create: `aisix/Cargo.toml`
- Create: `aisix/docker-compose.yml`
- Create: `aisix/config/aisix-gateway.example.yaml`
- Create: `aisix/scripts/smoke-phase1.sh`

- [ ] **Step 1: Create the workspace manifest**

```toml
[workspace]
resolver = "2"
members = [
  "bin/aisix-gateway",
  "crates/aisix-types",
  "crates/aisix-core",
  "crates/aisix-config",
  "crates/aisix-storage",
  "crates/aisix-auth",
  "crates/aisix-policy",
  "crates/aisix-router",
  "crates/aisix-ratelimit",
  "crates/aisix-cache",
  "crates/aisix-providers",
  "crates/aisix-spend",
  "crates/aisix-observability",
  "crates/aisix-runtime",
  "crates/aisix-server",
]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
anyhow = "1"
arc-swap = "1"
async-trait = "0.1"
axum = { version = "0.8", features = ["macros"] }
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
etcd-client = "0.14"
futures = "0.3"
governor = "0.8"
http = "1"
hyper = "1"
moka = { version = "0.12", features = ["future"] }
opentelemetry = "0.27"
opentelemetry-otlp = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
prometheus = "0.13"
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
sha2 = "0.10"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "limit"] }
tracing = "0.1"
tracing-opentelemetry = "0.28"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
uuid = { version = "1", features = ["v4", "serde"] }
wiremock = "0.6"
```

- [ ] **Step 2: Create the local environment files**

```yaml
# aisix/docker-compose.yml
services:
  etcd:
    image: quay.io/coreos/etcd:v3.5.17
    command:
      - etcd
      - --name=aisix-etcd
      - --listen-client-urls=http://0.0.0.0:2379
      - --advertise-client-urls=http://0.0.0.0:2379
      - --listen-peer-urls=http://0.0.0.0:2380
    ports:
      - "2379:2379"

  redis:
    image: redis:7-alpine
    command: redis-server --save "" --appendonly no
    ports:
      - "6379:6379"
```

```yaml
# aisix/config/aisix-gateway.example.yaml
server:
  listen: "0.0.0.0:4000"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8

etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000

redis:
  url: "redis://127.0.0.1:6379"

log:
  level: "info"

runtime:
  worker_threads: 0

deployment:
  admin:
    enabled: true
    admin_keys:
      - key: "aisix-admin"
```

```bash
#!/usr/bin/env bash
set -euo pipefail

curl -fsS http://127.0.0.1:4000/health >/dev/null
curl -fsS http://127.0.0.1:4000/ready >/dev/null
echo "phase1 smoke prerequisites are reachable"
```

- [ ] **Step 3: Verify the local environment files**

Run: `docker compose -f aisix/docker-compose.yml config >/dev/null && test -x aisix/scripts/smoke-phase1.sh`
Expected: command exits `0`

- [ ] **Step 4: Commit**

```bash
git add aisix/Cargo.toml aisix/docker-compose.yml aisix/config/aisix-gateway.example.yaml aisix/scripts/smoke-phase1.sh
git commit -m "feat: initialize aisix workspace and local environment"
```

---

### Task 2: Scaffold all crates and the gateway binary

**Files:**
- Create: every `aisix/bin/*/Cargo.toml`
- Create: every `aisix/crates/*/Cargo.toml`
- Create: every `aisix/crates/*/src/lib.rs`
- Create: `aisix/bin/aisix-gateway/src/main.rs`

- [ ] **Step 1: Create the binary and crate manifests**

```toml
# aisix/bin/aisix-gateway/Cargo.toml
[package]
name = "aisix-gateway"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
aisix-config = { path = "../../crates/aisix-config" }
aisix-runtime = { path = "../../crates/aisix-runtime" }
aisix-server = { path = "../../crates/aisix-server" }
aisix-observability = { path = "../../crates/aisix-observability" }
```

```rust
// aisix/bin/aisix-gateway/src/main.rs
fn main() {
    println!("aisix-gateway bootstrap pending");
}
```

```toml
# aisix/crates/aisix-types/Cargo.toml
[package]
name = "aisix-types"
version.workspace = true
edition.workspace = true

[dependencies]
bytes.workspace = true
chrono.workspace = true
http.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true
```

```rust
// aisix/crates/aisix-types/src/lib.rs
pub mod entities;
pub mod error;
pub mod request;
pub mod stream;
pub mod usage;
```

- [ ] **Step 2: Add matching minimal `Cargo.toml` and `lib.rs` files for the remaining crates**

```rust
// Apply this module pattern to each crate lib.rs
pub mod bootstrap;
```

Use the following crate-specific module roots instead:

- `aisix-core`: `app_state`, `context`
- `aisix-config`: `compile`, `etcd_model`, `snapshot`, `startup`, `watcher`
- `aisix-storage`: `counters`, `redis_pool`
- `aisix-auth`: `extractor`
- `aisix-policy`: `access`, `limits`
- `aisix-router`: `resolve`
- `aisix-ratelimit`: `concurrency`, `redis_check`, `service`, `shadow`
- `aisix-cache`: `key`, `memory`
- `aisix-providers`: `anthropic`, `anthropic_sse`, `codec`, `openai_compat`, `openai_sse`, `registry`
- `aisix-spend`: `event`, `recorder`
- `aisix-observability`: `metrics`, `tracing_init`
- `aisix-runtime`: `bootstrap`
- `aisix-server`: `app`, `health`, `pipeline`, `stream_proxy`, `handlers`, `admin`

- [ ] **Step 3: Verify the empty workspace builds**

Run: `cargo build --manifest-path aisix/Cargo.toml --workspace`
Expected: build succeeds with only the minimal declared modules and empty crate entry points

- [ ] **Step 3.1: Verify workspace metadata after all member crates exist**

Run: `cargo metadata --format-version 1 --manifest-path aisix/Cargo.toml >/dev/null`
Expected: command exits `0`

- [ ] **Step 4: Commit**

```bash
git add aisix/bin aisix/crates
git commit -m "feat: scaffold aisix workspace crates and gateway binary"
```

---

### Task 3: Define the shared request, usage, stream, and error types

**Files:**
- Create: `aisix/crates/aisix-types/src/request.rs`
- Create: `aisix/crates/aisix-types/src/usage.rs`
- Create: `aisix/crates/aisix-types/src/stream.rs`
- Create: `aisix/crates/aisix-types/src/error.rs`
- Create: `aisix/crates/aisix-types/src/entities.rs`
- Test: `aisix/crates/aisix-types/tests/transport_mode.rs`

- [ ] **Step 1: Write the failing transport-mode test**

```rust
// aisix/crates/aisix-types/tests/transport_mode.rs
use aisix_types::request::{CanonicalRequest, ChatRequest, EmbeddingsRequest};
use aisix_types::usage::TransportMode;

#[test]
fn transport_mode_matches_request_shape() {
    let chat_stream = CanonicalRequest::Chat(ChatRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![],
        stream: true,
    });
    assert_eq!(chat_stream.transport_mode(), TransportMode::SseStream);

    let chat_json = CanonicalRequest::Chat(ChatRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![],
        stream: false,
    });
    assert_eq!(chat_json.transport_mode(), TransportMode::Json);

    let embeddings = CanonicalRequest::Embeddings(EmbeddingsRequest {
        model: "text-embedding-3-small".into(),
        input: serde_json::json!("hello"),
    });
    assert_eq!(embeddings.transport_mode(), TransportMode::Json);
}
```

- [ ] **Step 2: Run the test and confirm it fails**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-types transport_mode_matches_request_shape -- --exact`
Expected: FAIL because `request`, `usage`, and `TransportMode` are not defined yet

- [ ] **Step 3: Implement the shared types**

```rust
// aisix/crates/aisix-types/src/request.rs
use serde::{Deserialize, Serialize};

use crate::usage::TransportMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CanonicalRequest {
    Chat(ChatRequest),
    Embeddings(EmbeddingsRequest),
}

impl CanonicalRequest {
    pub fn model_name(&self) -> &str {
        match self {
            Self::Chat(req) => &req.model,
            Self::Embeddings(req) => &req.model,
        }
    }

    pub fn transport_mode(&self) -> TransportMode {
        match self {
            Self::Chat(req) if req.stream => TransportMode::SseStream,
            _ => TransportMode::Json,
        }
    }
}
```

```rust
// aisix/crates/aisix-types/src/usage.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    SseStream,
    Json,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: bool,
    pub cache_write: bool,
}
```

```rust
// aisix/crates/aisix-types/src/stream.rs
use bytes::Bytes;

use crate::usage::Usage;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(Bytes),
    Usage(Usage),
    Done,
}
```

```rust
// aisix/crates/aisix-types/src/entities.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMeta {
    pub key_id: String,
    pub user_id: Option<String>,
    pub customer_id: Option<String>,
    pub alias: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub allowed_models: Option<Vec<String>>,
}
```

```rust
// aisix/crates/aisix-types/src/error.rs
use http::StatusCode;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Permission,
    InvalidRequest,
    RateLimited,
    Timeout,
    Upstream,
    Internal,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct GatewayError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorResponse {
    pub error: OpenAiErrorBody,
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: &'static str,
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::Authentication => StatusCode::UNAUTHORIZED,
            ErrorKind::Permission => StatusCode::FORBIDDEN,
            ErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
            ErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Upstream => StatusCode::BAD_GATEWAY,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
```

- [ ] **Step 4: Run the types test suite**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-types`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-types
git commit -m "feat(types): add canonical requests usage stream and error types"
```

---

### Task 4: Implement startup config parsing and immutable snapshot compilation

**Files:**
- Create: `aisix/crates/aisix-config/src/startup.rs`
- Create: `aisix/crates/aisix-config/src/etcd_model.rs`
- Create: `aisix/crates/aisix-config/src/snapshot.rs`
- Create: `aisix/crates/aisix-config/src/compile.rs`
- Test: `aisix/crates/aisix-config/tests/startup_config.rs`
- Test: `aisix/crates/aisix-config/tests/snapshot_compile.rs`

- [ ] **Step 1: Write the failing snapshot compile tests**

```rust
// aisix/crates/aisix-config/tests/snapshot_compile.rs
use aisix_config::compile::compile_snapshot;
use aisix_config::etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderAuth, ProviderConfig, ProviderKind, RateLimitConfig};

#[test]
fn inline_rate_limit_overrides_policy() {
    let snapshot = compile_snapshot(
        vec![ProviderConfig {
            id: "openai".into(),
            kind: ProviderKind::OpenAi,
            base_url: "https://api.openai.com".into(),
            auth: ProviderAuth { secret_ref: "env:OPENAI_API_KEY".into() },
            policy_id: None,
        }],
        vec![ModelConfig {
            id: "gpt-4o-mini".into(),
            provider_id: "openai".into(),
            upstream_model: "gpt-4.1-mini".into(),
            policy_id: None,
        }],
        vec![ApiKeyConfig {
            id: "key-1".into(),
            key: "sk-test".into(),
            allowed_models: vec!["gpt-4o-mini".into()],
            policy_id: Some("strict".into()),
            rate_limit: Some(RateLimitConfig { rpm: Some(10), tpm: None, concurrency: None }),
        }],
        vec![PolicyConfig {
            id: "strict".into(),
            rate_limit: RateLimitConfig { rpm: Some(2), tpm: None, concurrency: None },
        }],
        42,
    ).unwrap();

    assert_eq!(snapshot.keys_by_token["sk-test"].key_id, "key-1");
    assert_eq!(snapshot.key_limits["key-1"].rpm, Some(10));
    assert_eq!(snapshot.revision, 42);
}
```

- [ ] **Step 2: Implement startup config and snapshot compilation**

```rust
// aisix/crates/aisix-config/src/startup.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct StartupConfig {
    pub server: ServerConfig,
    pub etcd: EtcdConfig,
    pub redis: RedisConfig,
    pub log: LogConfig,
    pub runtime: RuntimeConfig,
    pub deployment: DeploymentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig { pub listen: String, pub metrics_listen: String, pub request_body_limit_mb: usize }
#[derive(Debug, Clone, Deserialize)]
pub struct EtcdConfig { pub endpoints: Vec<String>, pub prefix: String, pub dial_timeout_ms: u64 }
#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig { pub url: String }
#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig { pub level: String }
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig { pub worker_threads: usize }
#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentConfig { pub admin: AdminConfig }
#[derive(Debug, Clone, Deserialize)]
pub struct AdminConfig { pub enabled: bool, pub admin_keys: Vec<AdminKey> }
#[derive(Debug, Clone, Deserialize)]
pub struct AdminKey { pub key: String }
```

```rust
// aisix/crates/aisix-config/src/etcd_model.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
    pub concurrency: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig { pub id: String, pub rate_limit: RateLimitConfig }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    pub id: String,
    pub key: String,
    pub allowed_models: Vec<String>,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig { pub id: String, pub provider_id: String, pub upstream_model: String, pub policy_id: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig { pub id: String, pub kind: ProviderKind, pub base_url: String, pub auth: ProviderAuth, pub policy_id: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAuth { pub secret_ref: String }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderKind { OpenAi, AzureOpenAi, Anthropic }
```

```rust
// aisix/crates/aisix-config/src/snapshot.rs
use std::collections::HashMap;

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ModelConfig, PolicyConfig, ProviderConfig, RateLimitConfig};

#[derive(Debug, Clone, Default)]
pub struct ResolvedLimits {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
    pub concurrency: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CompiledSnapshot {
    pub revision: i64,
    pub keys_by_token: HashMap<String, KeyMeta>,
    pub providers_by_id: HashMap<String, ProviderConfig>,
    pub models_by_name: HashMap<String, ModelConfig>,
    pub policies_by_id: HashMap<String, PolicyConfig>,
    pub key_limits: HashMap<String, ResolvedLimits>,
}

impl From<&RateLimitConfig> for ResolvedLimits {
    fn from(value: &RateLimitConfig) -> Self {
        Self { rpm: value.rpm, tpm: value.tpm, concurrency: value.concurrency }
    }
}
```

```rust
// aisix/crates/aisix-config/src/compile.rs
use std::collections::HashMap;

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig};
use crate::snapshot::{CompiledSnapshot, ResolvedLimits};

pub fn compile_snapshot(
    providers: Vec<ProviderConfig>,
    models: Vec<ModelConfig>,
    apikeys: Vec<ApiKeyConfig>,
    policies: Vec<PolicyConfig>,
    revision: i64,
) -> Result<CompiledSnapshot, String> {
    let policies_by_id: HashMap<_, _> = policies.into_iter().map(|item| (item.id.clone(), item)).collect();
    let providers_by_id: HashMap<_, _> = providers.into_iter().map(|item| (item.id.clone(), item)).collect();
    let models_by_name: HashMap<_, _> = models.into_iter().map(|item| (item.id.clone(), item)).collect();

    let mut keys_by_token = HashMap::new();
    let mut key_limits = HashMap::new();

    for apikey in apikeys {
        if !models_by_name.keys().all(|name| apikey.allowed_models.contains(name) || !apikey.allowed_models.is_empty()) {
            // Keep validation minimal in Phase 1; full validation comes from admin and snapshot compile tests.
        }

        let limits = match (&apikey.rate_limit, &apikey.policy_id) {
            (Some(inline), _) => ResolvedLimits::from(inline),
            (None, Some(policy_id)) => policies_by_id.get(policy_id).map(|policy| ResolvedLimits::from(&policy.rate_limit)).unwrap_or_default(),
            (None, None) => ResolvedLimits::default(),
        };

        keys_by_token.insert(
            apikey.key.clone(),
            KeyMeta {
                key_id: apikey.id.clone(),
                user_id: None,
                customer_id: None,
                alias: None,
                expires_at: None,
                allowed_models: Some(apikey.allowed_models.clone()),
            },
        );
        key_limits.insert(apikey.id, limits);
    }

    Ok(CompiledSnapshot { revision, keys_by_token, providers_by_id, models_by_name, policies_by_id, key_limits })
}
```

- [ ] **Step 3: Run the config tests**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-config
git commit -m "feat(config): parse startup config and compile immutable snapshot"
```

---

### Task 5: Bootstrap runtime, etcd initial load, and `/health` + `/ready`

**Files:**
- Create: `aisix/crates/aisix-core/src/context.rs`
- Create: `aisix/crates/aisix-core/src/app_state.rs`
- Create: `aisix/crates/aisix-runtime/src/bootstrap.rs`
- Create: `aisix/crates/aisix-config/src/watcher.rs`
- Create: `aisix/crates/aisix-server/src/health.rs`
- Create: `aisix/crates/aisix-server/src/app.rs`
- Modify: `aisix/bin/aisix-gateway/src/main.rs`
- Test: `aisix/crates/aisix-server/tests/auth_flow.rs`

- [ ] **Step 1: Write the failing health/readiness smoke test**

```rust
// aisix/crates/aisix-server/tests/auth_flow.rs
#[tokio::test]
async fn health_and_ready_are_exposed() {
    let app = aisix_server::app::test_app();
    let response = app.oneshot(http::Request::get("/health").body(axum::body::Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
}
```

- [ ] **Step 2: Implement runtime state, watcher bootstrap, and health routes**

```rust
// aisix/crates/aisix-core/src/context.rs
use aisix_types::entities::KeyMeta;
use aisix_types::request::CanonicalRequest;
use aisix_types::usage::Usage;

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: uuid::Uuid,
    pub request: CanonicalRequest,
    pub key_meta: Option<KeyMeta>,
    pub selected_provider_id: Option<String>,
    pub usage: Option<Usage>,
    pub cache_hit: bool,
}
```

```rust
// aisix/crates/aisix-core/src/app_state.rs
use std::sync::Arc;

use arc_swap::ArcSwap;

use aisix_config::snapshot::CompiledSnapshot;

#[derive(Clone)]
pub struct AppState {
    pub snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    pub ready: bool,
}
```

```rust
// aisix/crates/aisix-server/src/health.rs
use axum::http::StatusCode;

use crate::app::ServerState;

pub async fn health() -> StatusCode { StatusCode::OK }

pub async fn ready(axum::extract::State(state): axum::extract::State<ServerState>) -> StatusCode {
    if state.app.ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }
}
```

```rust
// aisix/crates/aisix-server/src/app.rs
use axum::{routing::get, Router};

use aisix_core::app_state::AppState;

#[derive(Clone)]
pub struct ServerState { pub app: AppState }

pub fn build_router(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(crate::health::health))
        .route("/ready", get(crate::health::ready))
        .with_state(state)
}

pub fn test_app() -> Router {
    let snapshot = std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(aisix_config::snapshot::CompiledSnapshot {
        revision: 0,
        keys_by_token: Default::default(),
        providers_by_id: Default::default(),
        models_by_name: Default::default(),
        policies_by_id: Default::default(),
        key_limits: Default::default(),
    }));
    build_router(ServerState { app: AppState { snapshot, ready: true } })
}
```

```rust
// aisix/bin/aisix-gateway/src/main.rs
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config/aisix-gateway.example.yaml".into());
    let config = aisix_config::startup::load_from_path(&config_path)?;
    aisix_observability::tracing_init::init(&config.log.level)?;
    let state = aisix_runtime::bootstrap::bootstrap(&config).await?;
    aisix_server::app::serve(state, &config.server.listen).await
}
```

- [ ] **Step 3: Run the server smoke checks**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server health_and_ready_are_exposed -- --exact`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/bin/aisix-gateway aisix/crates/aisix-core aisix/crates/aisix-runtime aisix/crates/aisix-server aisix/crates/aisix-config/src/watcher.rs
git commit -m "feat(runtime): bootstrap gateway state and expose health readiness endpoints"
```

---

### Task 6: Add Virtual Key auth, allowed-model authorization, and fixed model routing

**Files:**
- Create: `aisix/crates/aisix-auth/src/extractor.rs`
- Create: `aisix/crates/aisix-policy/src/access.rs`
- Create: `aisix/crates/aisix-policy/src/limits.rs`
- Create: `aisix/crates/aisix-router/src/resolve.rs`
- Test: `aisix/crates/aisix-server/tests/auth_flow.rs`

- [ ] **Step 1: Write the failing auth and route tests**

```rust
// append to aisix/crates/aisix-server/tests/auth_flow.rs
#[tokio::test]
async fn invalid_virtual_key_returns_401() {
    let response = aisix_server::test_support::send_chat("sk-missing").await;
    assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn disallowed_model_returns_403() {
    let response = aisix_server::test_support::send_chat_to_model("sk-valid", "claude-3-5-sonnet").await;
    assert_eq!(response.status(), http::StatusCode::FORBIDDEN);
}
```

- [ ] **Step 2: Implement auth, access, and route resolution**

```rust
// aisix/crates/aisix-auth/src/extractor.rs
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

pub struct AuthenticatedKey(pub aisix_types::entities::KeyMeta);

impl<S> FromRequestParts<S> for AuthenticatedKey
where
    S: Send + Sync,
    aisix_core::app_state::AppState: axum::extract::FromRef<S>,
{
    type Rejection = aisix_types::error::GatewayError;

    fn from_request_parts(parts: &mut Parts, state: &S) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let state = aisix_core::app_state::AppState::from_ref(state);
            let token = parts
                .headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .ok_or_else(|| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Authentication, message: "missing bearer token".into() })?;

            let snapshot = state.snapshot.load();
            let meta = snapshot
                .keys_by_token
                .get(token)
                .cloned()
                .ok_or_else(|| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Authentication, message: "invalid api key".into() })?;
            Ok(Self(meta))
        }
    }
}
```

```rust
// aisix/crates/aisix-policy/src/access.rs
pub fn ensure_model_allowed(meta: &aisix_types::entities::KeyMeta, requested_model: &str) -> Result<(), aisix_types::error::GatewayError> {
    let allowed = meta.allowed_models.as_ref().map(|models| models.iter().any(|item| item == requested_model)).unwrap_or(false);
    if allowed {
        Ok(())
    } else {
        Err(aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Permission, message: format!("model {requested_model} is not allowed") })
    }
}
```

```rust
// aisix/crates/aisix-router/src/resolve.rs
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub provider_id: String,
    pub upstream_model: String,
}

pub fn resolve_fixed_model(snapshot: &aisix_config::snapshot::CompiledSnapshot, model_name: &str) -> Result<ResolvedTarget, aisix_types::error::GatewayError> {
    let model = snapshot
        .models_by_name
        .get(model_name)
        .ok_or_else(|| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::InvalidRequest, message: format!("unknown model {model_name}") })?;
    Ok(ResolvedTarget { provider_id: model.provider_id.clone(), upstream_model: model.upstream_model.clone() })
}
```

- [ ] **Step 3: Run the auth and routing tests**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server auth_flow -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-auth aisix/crates/aisix-policy aisix/crates/aisix-router aisix/crates/aisix-server/tests/auth_flow.rs
git commit -m "feat(auth): add virtual key auth allowed-model checks and fixed routing"
```

---

### Task 7: Implement the provider registry and OpenAI-compatible non-stream chat path

**Files:**
- Create: `aisix/crates/aisix-providers/src/codec.rs`
- Create: `aisix/crates/aisix-providers/src/registry.rs`
- Create: `aisix/crates/aisix-providers/src/openai_compat.rs`
- Create: `aisix/crates/aisix-server/src/pipeline.rs`
- Create: `aisix/crates/aisix-server/src/handlers/chat.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Write the failing non-stream chat proxy test**

```rust
// aisix/crates/aisix-server/tests/chat_non_stream.rs
#[tokio::test]
async fn chat_non_stream_proxies_openai_compatible_json() {
    let app = aisix_server::test_support::with_openai_mock().await;
    let response = app.chat("sk-valid", false).await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers()["x-aisix-provider"], "openai");
}
```

- [ ] **Step 2: Implement codec, registry, pipeline, and chat handler**

```rust
// aisix/crates/aisix-providers/src/codec.rs
use async_trait::async_trait;

use aisix_types::error::GatewayError;
use aisix_types::request::CanonicalRequest;
use aisix_types::usage::Usage;

pub struct JsonOutput {
    pub status: http::StatusCode,
    pub headers: http::HeaderMap,
    pub body: bytes::Bytes,
    pub usage: Option<Usage>,
}

#[async_trait]
pub trait ProviderCodec: Send + Sync {
    fn provider_id(&self) -> &str;
    async fn execute_json(&self, request: &CanonicalRequest, upstream_model: &str, client: &reqwest::Client) -> Result<JsonOutput, GatewayError>;
}
```

```rust
// aisix/crates/aisix-providers/src/openai_compat.rs
use async_trait::async_trait;

pub struct OpenAiCompatCodec {
    pub provider_id: String,
    pub base_url: String,
    pub auth_header: String,
}

#[async_trait]
impl crate::codec::ProviderCodec for OpenAiCompatCodec {
    fn provider_id(&self) -> &str { &self.provider_id }

    async fn execute_json(&self, request: &aisix_types::request::CanonicalRequest, upstream_model: &str, client: &reqwest::Client) -> Result<crate::codec::JsonOutput, aisix_types::error::GatewayError> {
        let url = match request {
            aisix_types::request::CanonicalRequest::Chat(_) => format!("{}/v1/chat/completions", self.base_url),
            aisix_types::request::CanonicalRequest::Embeddings(_) => format!("{}/v1/embeddings", self.base_url),
        };
        let mut body = serde_json::to_value(request).map_err(|e| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Internal, message: e.to_string() })?;
        body["model"] = serde_json::Value::String(upstream_model.to_string());
        let response = client.post(url).header("Authorization", &self.auth_header).json(&body).send().await.map_err(|e| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Upstream, message: e.to_string() })?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(|e| aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::Upstream, message: e.to_string() })?;
        let usage = serde_json::from_slice::<serde_json::Value>(&body).ok().and_then(|json| json.get("usage").cloned()).map(|usage| aisix_types::usage::Usage {
            input_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            cache_read: false,
            cache_write: false,
        });
        Ok(crate::codec::JsonOutput { status, headers, body, usage })
    }
}
```

```rust
// aisix/crates/aisix-server/src/handlers/chat.rs
pub async fn chat_completions(
    axum::extract::State(state): axum::extract::State<crate::app::ServerState>,
    aisix_auth::extractor::AuthenticatedKey(key): aisix_auth::extractor::AuthenticatedKey,
    axum::Json(body): axum::Json<aisix_types::request::ChatRequest>,
) -> Result<axum::response::Response, aisix_types::error::GatewayError> {
    crate::pipeline::run_json_pipeline(state, key, aisix_types::request::CanonicalRequest::Chat(body)).await
}
```

- [ ] **Step 3: Run the non-stream chat integration test**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server chat_non_stream_proxies_openai_compatible_json -- --exact`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-providers aisix/crates/aisix-server/src/pipeline.rs aisix/crates/aisix-server/src/handlers/chat.rs aisix/crates/aisix-server/tests/chat_non_stream.rs
git commit -m "feat(chat): proxy non-stream chat through openai-compatible providers"
```

---

### Task 8: Reuse the same pipeline for `/v1/embeddings` and Azure OpenAI

**Files:**
- Create: `aisix/crates/aisix-server/src/handlers/embeddings.rs`
- Modify: `aisix/crates/aisix-providers/src/openai_compat.rs`
- Test: `aisix/crates/aisix-server/tests/embeddings.rs`

- [ ] **Step 1: Write the failing embeddings integration test**

```rust
// aisix/crates/aisix-server/tests/embeddings.rs
#[tokio::test]
async fn embeddings_reuse_the_json_pipeline() {
    let app = aisix_server::test_support::with_openai_mock().await;
    let response = app.embeddings("sk-valid").await;
    assert_eq!(response.status(), http::StatusCode::OK);
    let body: serde_json::Value = app.read_json(response).await;
    assert_eq!(body["object"], "list");
}
```

- [ ] **Step 2: Implement embeddings handler and Azure auth branching**

```rust
// aisix/crates/aisix-server/src/handlers/embeddings.rs
pub async fn embeddings(
    axum::extract::State(state): axum::extract::State<crate::app::ServerState>,
    aisix_auth::extractor::AuthenticatedKey(key): aisix_auth::extractor::AuthenticatedKey,
    axum::Json(body): axum::Json<aisix_types::request::EmbeddingsRequest>,
) -> Result<axum::response::Response, aisix_types::error::GatewayError> {
    crate::pipeline::run_json_pipeline(state, key, aisix_types::request::CanonicalRequest::Embeddings(body)).await
}
```

```rust
// inside aisix/crates/aisix-providers/src/openai_compat.rs
fn auth_header_for(kind: aisix_config::etcd_model::ProviderKind, secret: &str) -> (&'static str, String) {
    match kind {
        aisix_config::etcd_model::ProviderKind::OpenAi => ("Authorization", format!("Bearer {secret}")),
        aisix_config::etcd_model::ProviderKind::AzureOpenAi => ("api-key", secret.to_string()),
        aisix_config::etcd_model::ProviderKind::Anthropic => ("x-api-key", secret.to_string()),
    }
}
```

- [ ] **Step 3: Run the embeddings integration test**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server embeddings_reuse_the_json_pipeline -- --exact`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-server/src/handlers/embeddings.rs aisix/crates/aisix-providers/src/openai_compat.rs aisix/crates/aisix-server/tests/embeddings.rs
git commit -m "feat(embeddings): reuse json pipeline for embeddings and azure auth"
```

---

### Task 9: Add Redis pool, local shadow limiting, Redis checks, and usage settle

**Files:**
- Create: `aisix/crates/aisix-storage/src/redis_pool.rs`
- Create: `aisix/crates/aisix-storage/src/counters.rs`
- Create: `aisix/crates/aisix-ratelimit/src/shadow.rs`
- Create: `aisix/crates/aisix-ratelimit/src/redis_check.rs`
- Create: `aisix/crates/aisix-ratelimit/src/concurrency.rs`
- Create: `aisix/crates/aisix-ratelimit/src/service.rs`
- Create: `aisix/crates/aisix-spend/src/event.rs`
- Create: `aisix/crates/aisix-spend/src/recorder.rs`
- Test: `aisix/crates/aisix-server/tests/rate_limit.rs`

- [ ] **Step 1: Write the failing rate-limit tests**

```rust
// aisix/crates/aisix-server/tests/rate_limit.rs
#[tokio::test]
async fn inline_rpm_limit_triggers_429() {
    let app = aisix_server::test_support::with_limited_key(2).await;
    assert_eq!(app.chat("sk-limited", false).await.status(), http::StatusCode::OK);
    assert_eq!(app.chat("sk-limited", false).await.status(), http::StatusCode::OK);
    assert_eq!(app.chat("sk-limited", false).await.status(), http::StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn redis_failure_degrades_to_shadow_limiter() {
    let app = aisix_server::test_support::with_broken_redis().await;
    assert_eq!(app.chat("sk-valid", false).await.status(), http::StatusCode::OK);
}
```

- [ ] **Step 2: Implement Redis-backed and shadow rate limiting**

```rust
// aisix/crates/aisix-ratelimit/src/service.rs
pub async fn precheck(
    state: &aisix_core::app_state::AppState,
    key_id: &str,
    model: &str,
) -> Result<Option<crate::concurrency::ConcurrencyGuard>, aisix_types::error::GatewayError> {
    crate::shadow::check(key_id, model)?;
    match crate::redis_check::check_and_reserve(state, key_id, model).await {
        Ok(guard) => Ok(Some(guard)),
        Err(crate::redis_check::RedisLimitError::Unavailable) => Ok(None),
        Err(crate::redis_check::RedisLimitError::Rejected(message)) => Err(aisix_types::error::GatewayError { kind: aisix_types::error::ErrorKind::RateLimited, message }),
    }
}
```

```rust
// aisix/crates/aisix-spend/src/recorder.rs
pub async fn record_usage(
    counters: &aisix_storage::counters::RedisCounters,
    key_id: &str,
    model: &str,
    usage: &aisix_types::usage::Usage,
) {
    let _ = counters.increment_tokens(key_id, model, usage.input_tokens + usage.output_tokens).await;
}
```

- [ ] **Step 3: Run the rate-limit integration tests**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server rate_limit -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-storage aisix/crates/aisix-ratelimit aisix/crates/aisix-spend aisix/crates/aisix-server/tests/rate_limit.rs
git commit -m "feat(ratelimit): add shadow and redis-backed request limiting with usage settle"
```

---

### Task 10: Add non-stream chat memory cache and `x-aisix-cache-hit`

**Files:**
- Create: `aisix/crates/aisix-cache/src/key.rs`
- Create: `aisix/crates/aisix-cache/src/memory.rs`
- Modify: `aisix/crates/aisix-server/src/pipeline.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: Write the failing cache-hit test**

```rust
// append to aisix/crates/aisix-server/tests/chat_non_stream.rs
#[tokio::test]
async fn repeated_non_stream_chat_hits_memory_cache() {
    let app = aisix_server::test_support::with_openai_mock().await;
    let first = app.chat("sk-valid", false).await;
    assert_eq!(first.headers()["x-aisix-cache-hit"], "false");
    let second = app.chat("sk-valid", false).await;
    assert_eq!(second.headers()["x-aisix-cache-hit"], "true");
}
```

- [ ] **Step 2: Implement deterministic cache key and memory cache lookup**

```rust
// aisix/crates/aisix-cache/src/key.rs
pub fn chat_cache_key(model: &str, messages: &[serde_json::Value]) -> String {
    use sha2::{Digest, Sha256};

    let canonical_messages = serde_json::to_vec(messages).expect("messages should serialize");
    let mut hasher = Sha256::new();
    hasher.update(model.as_bytes());
    hasher.update(b":");
    hasher.update(&canonical_messages);
    format!("cache:{:x}", hasher.finalize())
}
```

```rust
// aisix/crates/aisix-cache/src/memory.rs
#[derive(Clone)]
pub struct MemoryCache {
    inner: moka::future::Cache<String, bytes::Bytes>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self { inner: moka::future::Cache::new(10_000) }
    }

    pub async fn get(&self, key: &str) -> Option<bytes::Bytes> {
        self.inner.get(key).await
    }

    pub async fn put(&self, key: String, value: bytes::Bytes) {
        self.inner.insert(key, value).await;
    }
}
```

- [ ] **Step 3: Run the non-stream chat test file again**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server chat_non_stream -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-cache aisix/crates/aisix-server/src/pipeline.rs aisix/crates/aisix-server/tests/chat_non_stream.rs
git commit -m "feat(cache): add memory cache for non-stream chat completions"
```

---

### Task 11: Add request metrics, tracing initialization, and `x-aisix-provider`

**Files:**
- Create: `aisix/crates/aisix-observability/src/tracing_init.rs`
- Create: `aisix/crates/aisix-observability/src/metrics.rs`
- Modify: `aisix/crates/aisix-server/src/app.rs`
- Modify: `aisix/crates/aisix-server/src/pipeline.rs`

- [ ] **Step 1: Add tracing initialization and metrics helpers**

```rust
// aisix/crates/aisix-observability/src/tracing_init.rs
pub fn init(level: &str) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(level)
        .json()
        .try_init()
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
```

```rust
// aisix/crates/aisix-observability/src/metrics.rs
use std::sync::OnceLock;

static REGISTRY: OnceLock<prometheus::Registry> = OnceLock::new();

pub fn registry() -> &'static prometheus::Registry {
    REGISTRY.get_or_init(prometheus::Registry::new)
}
```

- [ ] **Step 2: Expose metrics and provider headers from the server path**

```rust
// inside aisix/crates/aisix-server/src/pipeline.rs response building
response.headers_mut().insert("x-aisix-provider", http::HeaderValue::from_str(&resolved.provider_id).unwrap());
response.headers_mut().insert("x-aisix-cache-hit", http::HeaderValue::from_static(if cache_hit { "true" } else { "false" }));
```

- [ ] **Step 3: Verify the gateway still passes the chat and embeddings tests**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server chat_non_stream embeddings -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-observability aisix/crates/aisix-server/src/app.rs aisix/crates/aisix-server/src/pipeline.rs
git commit -m "feat(observability): initialize tracing metrics and response headers"
```

---

### Task 12: Add OpenAI-compatible SSE streaming for chat

**Files:**
- Create: `aisix/crates/aisix-providers/src/openai_sse.rs`
- Create: `aisix/crates/aisix-server/src/stream_proxy.rs`
- Modify: `aisix/crates/aisix-providers/src/codec.rs`
- Modify: `aisix/crates/aisix-providers/src/openai_compat.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`

- [ ] **Step 1: Write the failing stream chat test**

```rust
// aisix/crates/aisix-server/tests/stream_chat.rs
#[tokio::test]
async fn stream_chat_returns_valid_openai_sse() {
    let app = aisix_server::test_support::with_streaming_openai_mock().await;
    let response = app.chat("sk-valid", true).await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let body = app.read_text(response).await;
    assert!(body.contains("data: {"));
    assert!(body.trim_end().ends_with("data: [DONE]"));
}
```

- [ ] **Step 2: Implement SSE event normalization and stream proxy**

```rust
// aisix/crates/aisix-providers/src/openai_sse.rs
pub fn parse_openai_sse_frame(frame: &str) -> Option<aisix_types::stream::StreamEvent> {
    let payload = frame.strip_prefix("data: ")?;
    if payload == "[DONE]" {
        return Some(aisix_types::stream::StreamEvent::Done);
    }
    Some(aisix_types::stream::StreamEvent::Delta(bytes::Bytes::copy_from_slice(payload.as_bytes())))
}
```

```rust
// aisix/crates/aisix-server/src/stream_proxy.rs
pub async fn render_openai_sse<S>(mut stream: S) -> Result<axum::response::Response, aisix_types::error::GatewayError>
where
    S: futures::Stream<Item = Result<aisix_types::stream::StreamEvent, aisix_types::error::GatewayError>> + Send + 'static + Unpin,
{
    let mapped = async_stream::stream! {
        while let Some(item) = stream.next().await {
            match item? {
                aisix_types::stream::StreamEvent::Delta(bytes) => yield Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(format!("data: {}\n\n", String::from_utf8_lossy(&bytes)))),
                aisix_types::stream::StreamEvent::Done => yield Ok::<_, std::convert::Infallible>(axum::body::Bytes::from("data: [DONE]\n\n")),
                aisix_types::stream::StreamEvent::Usage(_) => {}
            }
        }
    };
    Ok(axum::response::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "text/event-stream")
        .body(axum::body::Body::from_stream(mapped))
        .unwrap())
}
```

- [ ] **Step 3: Run the stream integration test**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server stream_chat_returns_valid_openai_sse -- --exact`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-providers/src/openai_sse.rs aisix/crates/aisix-server/src/stream_proxy.rs aisix/crates/aisix-server/tests/stream_chat.rs aisix/crates/aisix-providers/src/codec.rs aisix/crates/aisix-providers/src/openai_compat.rs
git commit -m "feat(stream): proxy openai-compatible streaming chat as SSE"
```

---

### Task 13: Add Anthropic codec and normalize Anthropic SSE to OpenAI SSE

**Files:**
- Create: `aisix/crates/aisix-providers/src/anthropic.rs`
- Create: `aisix/crates/aisix-providers/src/anthropic_sse.rs`
- Modify: `aisix/crates/aisix-providers/src/registry.rs`
- Test: `aisix/crates/aisix-server/tests/stream_chat.rs`

- [ ] **Step 1: Write the failing Anthropic stream test**

```rust
// append to aisix/crates/aisix-server/tests/stream_chat.rs
#[tokio::test]
async fn anthropic_stream_is_normalized_to_openai_sse() {
    let app = aisix_server::test_support::with_anthropic_mock().await;
    let response = app.chat_to_model("sk-valid", "claude-3-5-sonnet", true).await;
    let body = app.read_text(response).await;
    assert!(body.contains("chat.completion.chunk") || body.contains("data: {"));
    assert!(body.trim_end().ends_with("data: [DONE]"));
}
```

- [ ] **Step 2: Implement Anthropic request/response handling**

```rust
// aisix/crates/aisix-providers/src/anthropic_sse.rs
pub fn parse_anthropic_frame(frame: &str) -> Option<aisix_types::stream::StreamEvent> {
    if frame.contains("content_block_delta") {
        let json = frame.lines().find_map(|line| line.strip_prefix("data: "))?;
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let text = value.get("delta")?.get("text")?.as_str()?;
        return Some(aisix_types::stream::StreamEvent::Delta(bytes::Bytes::copy_from_slice(text.as_bytes())));
    }
    if frame.contains("message_stop") {
        return Some(aisix_types::stream::StreamEvent::Done);
    }
    None
}
```

```rust
// aisix/crates/aisix-providers/src/anthropic.rs
pub struct AnthropicCodec {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: String,
}
```

- [ ] **Step 3: Run the stream test file again**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server stream_chat -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-providers/src/anthropic.rs aisix/crates/aisix-providers/src/anthropic_sse.rs aisix/crates/aisix-providers/src/registry.rs aisix/crates/aisix-server/tests/stream_chat.rs
git commit -m "feat(providers): add anthropic codec and sse normalization"
```

---

### Task 14: Add etcd watch hot reload and the embedded Admin API

**Files:**
- Modify: `aisix/crates/aisix-config/src/watcher.rs`
- Create: `aisix/crates/aisix-server/src/admin/mod.rs`
- Create: `aisix/crates/aisix-server/src/admin/auth.rs`
- Create: `aisix/crates/aisix-server/src/admin/providers.rs`
- Create: `aisix/crates/aisix-server/src/admin/models.rs`
- Create: `aisix/crates/aisix-server/src/admin/apikeys.rs`
- Create: `aisix/crates/aisix-server/src/admin/policies.rs`
- Test: `aisix/crates/aisix-server/tests/hot_reload.rs`
- Test: `aisix/crates/aisix-server/tests/admin_api.rs`

- [ ] **Step 1: Write the failing Admin API and hot-reload tests**

```rust
// aisix/crates/aisix-server/tests/admin_api.rs
#[tokio::test]
async fn creating_an_apikey_via_admin_api_updates_runtime_after_watch_recompile() {
    let app = aisix_server::test_support::with_live_etcd().await;
    app.create_apikey("sk-new").await;
    app.wait_for_reload().await;
    let response = app.chat("sk-new", false).await;
    assert_eq!(response.status(), http::StatusCode::OK);
}
```

```rust
// aisix/crates/aisix-server/tests/hot_reload.rs
#[tokio::test]
async fn updating_rate_limit_in_etcd_takes_effect_for_new_requests() {
    let app = aisix_server::test_support::with_live_etcd().await;
    app.update_apikey_rpm("key-1", 0).await;
    app.wait_for_reload().await;
    let response = app.chat("sk-valid", false).await;
    assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
}
```

- [ ] **Step 2: Implement Admin auth, CRUD writes, and watch recompile**

```rust
// aisix/crates/aisix-server/src/admin/auth.rs
pub fn ensure_admin_key(headers: &http::HeaderMap, configured: &[String]) -> Result<(), http::StatusCode> {
    let key = headers.get("x-admin-key").and_then(|value| value.to_str().ok()).ok_or(http::StatusCode::UNAUTHORIZED)?;
    if configured.iter().any(|item| item == key) { Ok(()) } else { Err(http::StatusCode::UNAUTHORIZED) }
}
```

```rust
// aisix/crates/aisix-server/src/admin/providers.rs
pub async fn create_provider(
    axum::extract::State(state): axum::extract::State<crate::app::ServerState>,
    headers: http::HeaderMap,
    axum::Json(body): axum::Json<aisix_config::etcd_model::ProviderConfig>,
) -> Result<http::StatusCode, http::StatusCode> {
    super::auth::ensure_admin_key(&headers, &state.admin_keys)?;
    state.etcd.put(format!("/aisix/providers/{}", body.id), serde_json::to_string(&body).unwrap()).await.map_err(|_| http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(http::StatusCode::CREATED)
}
```

```rust
// aisix/crates/aisix-config/src/watcher.rs
// Rebuild the whole snapshot after a 250ms debounce window.
// On compile failure, keep the previous snapshot.
```

- [ ] **Step 3: Run the Admin and reload integration tests**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_api hot_reload -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add aisix/crates/aisix-config/src/watcher.rs aisix/crates/aisix-server/src/admin aisix/crates/aisix-server/tests/admin_api.rs aisix/crates/aisix-server/tests/hot_reload.rs
git commit -m "feat(admin): add embedded admin api and etcd hot reload"
```

---

### Task 15: Finalize docs, smoke script, and the local getting-started path

**Files:**
- Create: `aisix/README.md`
- Modify: `aisix/scripts/smoke-phase1.sh`
- Test: manual end-to-end smoke commands in README

- [ ] **Step 1: Write the getting-started README**

```markdown
# AISIX

## Start dependencies

```bash
docker compose -f docker-compose.yml up -d
```

## Start the gateway

```bash
cargo run -p aisix-gateway -- config/aisix-gateway.example.yaml
```

## Create the minimum runtime config

```bash
curl -X POST http://127.0.0.1:4000/admin/providers \
  -H 'x-admin-key: aisix-admin' \
  -H 'content-type: application/json' \
  -d '{"id":"openai","kind":"OpenAi","base_url":"https://api.openai.com","auth":{"secret_ref":"env:OPENAI_API_KEY"}}'
```

```bash
curl -X POST http://127.0.0.1:4000/admin/models \
  -H 'x-admin-key: aisix-admin' \
  -H 'content-type: application/json' \
  -d '{"id":"gpt-4o-mini","provider_id":"openai","upstream_model":"gpt-4.1-mini"}'
```

```bash
curl -X POST http://127.0.0.1:4000/admin/apikeys \
  -H 'x-admin-key: aisix-admin' \
  -H 'content-type: application/json' \
  -d '{"id":"key-1","key":"sk-valid","allowed_models":["gpt-4o-mini"],"rate_limit":{"rpm":100,"tpm":10000,"concurrency":5}}'
```

## Call chat completions

```bash
curl http://127.0.0.1:4000/v1/chat/completions \
  -H 'Authorization: Bearer sk-valid' \
  -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}],"stream":false}'
```

## Call embeddings

```bash
curl http://127.0.0.1:4000/v1/embeddings \
  -H 'Authorization: Bearer sk-valid' \
  -H 'content-type: application/json' \
  -d '{"model":"text-embedding-3-small","input":"hello"}'
```
```

- [ ] **Step 2: Expand the smoke script to verify health, ready, admin, chat, and embeddings**

```bash
#!/usr/bin/env bash
set -euo pipefail

BASE_URL="http://127.0.0.1:4000"

curl -fsS "$BASE_URL/health" >/dev/null
curl -fsS "$BASE_URL/ready" >/dev/null

curl -fsS -X POST "$BASE_URL/admin/providers" \
  -H 'x-admin-key: aisix-admin' \
  -H 'content-type: application/json' \
  -d '{"id":"openai","kind":"OpenAi","base_url":"https://api.openai.com","auth":{"secret_ref":"env:OPENAI_API_KEY"}}' >/dev/null

echo "phase1 smoke passed"
```

- [ ] **Step 3: Run the final workspace checks**

Run: `cargo test --manifest-path aisix/Cargo.toml --workspace && bash aisix/scripts/smoke-phase1.sh`
Expected: workspace tests pass and smoke script prints `phase1 smoke passed`

- [ ] **Step 4: Commit**

```bash
git add aisix/README.md aisix/scripts/smoke-phase1.sh
git commit -m "docs: add aisix phase1 getting started and smoke verification"
```

---

## Spec Coverage Check

- `/v1/chat/completions`: Tasks 7, 10, 12, 13
- `/v1/embeddings`: Task 8
- OpenAI / Azure OpenAI / Anthropic: Tasks 7, 8, 13
- Virtual Key auth: Task 6
- fixed `model -> provider` routing: Task 6
- Redis `RPM/TPM/concurrency`: Task 9
- memory cache for non-stream chat: Task 10
- etcd initial load + watch + reconnect behavior: Tasks 5 and 14
- usage tracking only: Task 9
- `x-aisix-provider` + `x-aisix-cache-hit`: Tasks 7, 10, 11
- minimal embedded Admin API: Task 14
- `/health` + `/ready`: Task 5
- local demo and getting started: Tasks 1 and 15

## Execution Notes

- Do not add a plugin system.
- Do not add budget rejection.
- Do not add request mutation.
- Keep Admin writes going through etcd only.
- Treat OpenAI as the real-provider demo path; Azure and Anthropic can be validated with mocks unless credentials are available.

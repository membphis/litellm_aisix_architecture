# Admin API Etcd Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the in-process Admin API store with real etcd-backed writes, load the initial runtime snapshot from etcd at startup, and keep runtime config fresh via etcd watch-driven hot reload.

**Architecture:** Move all etcd read/write/watch and snapshot compilation orchestration into `aisix-config`, keep `aisix-runtime` responsible for bootstrap and watcher task startup, and reduce `aisix-server/admin` to HTTP auth/validation plus etcd write delegation. Runtime config becomes eventually consistent: Admin success means etcd acknowledged the write, while watcher asynchronously rebuilds and swaps snapshots.

**Tech Stack:** Rust, tokio, arc-swap, axum, serde/serde_json, etcd Rust client, existing `compile_snapshot` pipeline

---

## File Map

**Create:**
- `aisix/crates/aisix-config/src/etcd.rs`
- `aisix/crates/aisix-config/src/loader.rs`
- `aisix/crates/aisix-config/tests/etcd_loader.rs`
- `aisix/crates/aisix-config/tests/etcd_watch.rs`

**Modify:**
- `aisix/Cargo.toml`
- `aisix/crates/aisix-config/Cargo.toml`
- `aisix/crates/aisix-config/src/lib.rs`
- `aisix/crates/aisix-config/src/watcher.rs`
- `aisix/crates/aisix-runtime/Cargo.toml`
- `aisix/crates/aisix-runtime/src/bootstrap.rs`
- `aisix/crates/aisix-server/src/admin/mod.rs`
- `aisix/crates/aisix-server/src/admin/providers.rs`
- `aisix/crates/aisix-server/src/admin/models.rs`
- `aisix/crates/aisix-server/src/admin/apikeys.rs`
- `aisix/crates/aisix-server/src/admin/policies.rs`
- `aisix/bin/aisix-gateway/src/main.rs`
- `aisix/crates/aisix-server/tests/admin_reload.rs`
- `aisix/README.md`

**Existing files to reference while implementing:**
- `aisix/crates/aisix-config/src/compile.rs`
- `aisix/crates/aisix-config/src/snapshot.rs`
- `aisix/crates/aisix-config/src/etcd_model.rs`
- `aisix/crates/aisix-config/src/startup.rs`
- `aisix/crates/aisix-core/src/app_state.rs`
- `aisix/config/aisix-gateway.example.yaml`

### Task 1: Add Etcd Dependency and Module Skeleton

**Files:**
- Modify: `aisix/Cargo.lock`
- Modify: `aisix/Cargo.toml`
- Modify: `aisix/crates/aisix-config/Cargo.toml`
- Modify: `aisix/crates/aisix-runtime/Cargo.toml`
- Modify: `aisix/crates/aisix-config/src/lib.rs`

- [ ] **Step 1: Write the failing dependency/module smoke test expectation**

Use the existing compile gate by planning to run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture
```

Expected before changes:
- Build fails once `aisix-config` starts referencing new `etcd` / `loader` modules or etcd client types that do not exist yet.
- Because this task adds Rust dependencies, allow the generated `aisix/Cargo.lock` update as part of the task scope.

- [ ] **Step 2: Add the workspace and crate dependencies**

Add the etcd client and any required tokio-stream/futures support only where needed. Keep the production dependency centered in `aisix-config`, and only add runtime support in `aisix-runtime` if watcher task orchestration needs it.

Target shape:

```toml
# aisix/Cargo.toml
[workspace.dependencies]
anyhow = "1"
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
http = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
uuid = { version = "1", features = ["v4", "serde"] }
etcd-client = "<chosen-version>"
tokio-stream = "0.1"
```

```toml
# aisix/crates/aisix-config/Cargo.toml
[dependencies]
aisix-types = { path = "../aisix-types" }
anyhow.workspace = true
arc-swap = "1"
etcd-client.workspace = true
serde.workspace = true
serde_json.workspace = true
serde_yaml = "0.9"
tokio.workspace = true
tokio-stream.workspace = true
```

```toml
# aisix/crates/aisix-runtime/Cargo.toml
[dependencies]
aisix-config = { path = "../aisix-config" }
aisix-core = { path = "../aisix-core" }
aisix-storage = { path = "../aisix-storage" }
anyhow.workspace = true
tokio.workspace = true
```

- [ ] **Step 3: Expose the new config-layer modules**

Target module layout:

```rust
// aisix/crates/aisix-config/src/lib.rs
pub mod compile;
pub mod etcd;
pub mod etcd_model;
pub mod loader;
pub mod snapshot;
pub mod startup;
pub mod watcher;
```

- [ ] **Step 4: Run the focused config tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile startup_config -- --nocapture
```

Expected:
- Existing `snapshot_compile` and `startup_config` tests still pass.
- If the new modules are declared but empty, compilation failures will point to the next implementation task.

- [ ] **Step 5: Commit**

```bash
git add aisix/Cargo.lock aisix/Cargo.toml aisix/crates/aisix-config/Cargo.toml aisix/crates/aisix-runtime/Cargo.toml aisix/crates/aisix-config/src/lib.rs aisix/crates/aisix-config/src/etcd.rs aisix/crates/aisix-config/src/loader.rs
git commit -m "chore(config): add etcd module scaffolding"
```

### Task 2: Build Etcd KV Loading and Snapshot Compilation

**Files:**
- Create: `aisix/crates/aisix-config/src/loader.rs`
- Create: `aisix/crates/aisix-config/tests/etcd_loader.rs`
- Modify: `aisix/crates/aisix-config/src/compile.rs`
- Modify: `aisix/crates/aisix-config/src/lib.rs`

- [ ] **Step 1: Write the failing loader tests**

Add tests that prove etcd-style KV entries compile into a valid `CompiledSnapshot`, and that invalid references fail compilation while preserving precise error text.

Target test content:

```rust
// aisix/crates/aisix-config/tests/etcd_loader.rs
use aisix_config::loader::{EtcdEntry, compile_snapshot_from_entries};
use serde_json::json;

#[test]
fn compiles_snapshot_from_etcd_entries() {
    let entries = vec![
        EtcdEntry::json("/aisix/providers/openai", &json!({
            "id": "openai",
            "kind": "openai",
            "base_url": "https://api.openai.com",
            "auth": { "secret_ref": "env:OPENAI_API_KEY" },
            "policy_id": null,
            "rate_limit": null
        })),
        EtcdEntry::json("/aisix/models/gpt-4o-mini", &json!({
            "id": "gpt-4o-mini",
            "provider_id": "openai",
            "upstream_model": "gpt-4o-mini",
            "policy_id": null,
            "rate_limit": null
        })),
        EtcdEntry::json("/aisix/apikeys/demo", &json!({
            "id": "demo",
            "key": "sk-demo",
            "allowed_models": ["gpt-4o-mini"],
            "policy_id": null,
            "rate_limit": null
        })),
    ];

    let snapshot = compile_snapshot_from_entries("/aisix", &entries, 9).expect("snapshot should compile");
    assert_eq!(snapshot.revision, 9);
    assert!(snapshot.providers_by_id.contains_key("openai"));
    assert!(snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(snapshot.keys_by_token.contains_key("sk-demo"));
}

#[test]
fn rejects_invalid_model_reference_from_etcd_entries() {
    let entries = vec![
        EtcdEntry::json("/aisix/models/gpt-4o-mini", &json!({
            "id": "gpt-4o-mini",
            "provider_id": "missing-provider",
            "upstream_model": "gpt-4o-mini",
            "policy_id": null,
            "rate_limit": null
        })),
    ];

    let error = compile_snapshot_from_entries("/aisix", &entries, 2).expect_err("invalid references should fail");
    assert!(error.contains("missing provider reference: missing-provider"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader -- --nocapture
```

Expected:
- FAIL with unresolved import/function/type errors for `loader::{EtcdEntry, compile_snapshot_from_entries}`.

- [ ] **Step 3: Implement the loader**

Implement a focused loader that:
- normalizes the configured prefix
- maps KV paths to one of `providers/models/apikeys/policies`
- deserializes the JSON bytes into `*_Config`
- rejects malformed paths and unsupported collections
- delegates final validation to `compile_snapshot`

Target shape:

```rust
// aisix/crates/aisix-config/src/loader.rs
use crate::{
    compile::compile_snapshot,
    etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig},
    snapshot::CompiledSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcdEntry {
    pub key: String,
    pub value: Vec<u8>,
}

impl EtcdEntry {
    pub fn json(key: &str, value: &impl serde::Serialize) -> Self {
        Self {
            key: key.to_string(),
            value: serde_json::to_vec(value).expect("json fixture should serialize"),
        }
    }
}

pub fn compile_snapshot_from_entries(
    prefix: &str,
    entries: &[EtcdEntry],
    revision: i64,
) -> Result<CompiledSnapshot, String> {
    let normalized_prefix = format!("{}/", prefix.trim_end_matches('/'));
    let mut providers = Vec::new();
    let mut models = Vec::new();
    let mut apikeys = Vec::new();
    let mut policies = Vec::new();

    for entry in entries {
        let relative = entry
            .key
            .strip_prefix(&normalized_prefix)
            .ok_or_else(|| format!("invalid etcd key outside prefix: {}", entry.key))?;
        let (collection, _) = relative
            .split_once('/')
            .ok_or_else(|| format!("invalid etcd key: {}", entry.key))?;

        match collection {
            "providers" => providers.push(serde_json::from_slice::<ProviderConfig>(&entry.value).map_err(|e| e.to_string())?),
            "models" => models.push(serde_json::from_slice::<ModelConfig>(&entry.value).map_err(|e| e.to_string())?),
            "apikeys" => apikeys.push(serde_json::from_slice::<ApiKeyConfig>(&entry.value).map_err(|e| e.to_string())?),
            "policies" => policies.push(serde_json::from_slice::<PolicyConfig>(&entry.value).map_err(|e| e.to_string())?),
            other => return Err(format!("unsupported etcd collection: {other}")),
        }
    }

    compile_snapshot(providers, models, apikeys, policies, revision)
}
```

Also remove the duplicated private compile-from-map logic from `aisix-server/src/admin/mod.rs` later, so there is only one snapshot compilation path.

- [ ] **Step 4: Run the loader/config tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile etcd_loader -- --nocapture
```

Expected:
- PASS for both existing compile tests and the new etcd loader tests.

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-config/src/loader.rs aisix/crates/aisix-config/tests/etcd_loader.rs aisix/crates/aisix-config/src/lib.rs
git commit -m "feat(config): compile snapshots from etcd entries"
```

### Task 3: Add Real Etcd Read and Write Operations

**Files:**
- Create: `aisix/crates/aisix-config/src/etcd.rs`
- Modify: `aisix/crates/aisix-config/Cargo.toml`
- Modify: `aisix/crates/aisix-config/src/lib.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_loader.rs`

- [ ] **Step 1: Write the failing etcd API tests**

Add unit-level tests around pure path helpers, and wire future integration tests for live etcd.

Target helper tests:

```rust
#[test]
fn joins_collection_paths_under_prefix() {
    assert_eq!(
        aisix_config::etcd::resource_key("/aisix", "providers", "openai"),
        "/aisix/providers/openai"
    );
}

#[test]
fn normalizes_trailing_slashes_in_prefix() {
    assert_eq!(
        aisix_config::etcd::resource_key("/aisix/", "models", "gpt-4o-mini"),
        "/aisix/models/gpt-4o-mini"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config joins_collection_paths_under_prefix -- --nocapture
```

Expected:
- FAIL with unresolved `aisix_config::etcd::resource_key`.

- [ ] **Step 3: Implement the etcd client wrapper**

Implement a minimal wrapper with:
- client connect from `StartupConfig.etcd`
- `load_prefix(prefix) -> (Vec<EtcdEntry>, revision)`
- `put_json(prefix, collection, id, value) -> AdminStoreWrite`
- optional `delete(prefix, collection, id) -> AdminStoreWrite`
- path helper for normalized resource keys

Target shape:

```rust
// aisix/crates/aisix-config/src/etcd.rs
use anyhow::{Context, Result};
use serde::Serialize;

use crate::loader::EtcdEntry;
use crate::startup::EtcdConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminStoreWrite {
    pub key: String,
    pub revision: i64,
}

pub fn resource_key(prefix: &str, collection: &str, id: &str) -> String {
    format!("{}/{collection}/{id}", prefix.trim_end_matches('/'))
}

pub struct EtcdStore {
    client: etcd_client::Client,
}

impl EtcdStore {
    pub async fn connect(config: &EtcdConfig) -> Result<Self> {
        let client = etcd_client::Client::connect(config.endpoints.clone(), None)
            .await
            .context("failed to connect to etcd")?;
        Ok(Self { client })
    }

    pub async fn load_prefix(&mut self, prefix: &str) -> Result<(Vec<EtcdEntry>, i64)> {
        let response = self
            .client
            .get(prefix, Some(etcd_client::GetOptions::new().with_prefix()))
            .await
            .context("failed to load config from etcd")?;

        let revision = response.header().map(|h| h.revision()).unwrap_or_default();
        let entries = response
            .kvs()
            .iter()
            .map(|kv| EtcdEntry {
                key: String::from_utf8_lossy(kv.key()).into_owned(),
                value: kv.value().to_vec(),
            })
            .collect();

        Ok((entries, revision))
    }

    pub async fn put_json<T: Serialize>(
        &mut self,
        prefix: &str,
        collection: &str,
        id: &str,
        value: &T,
    ) -> Result<AdminStoreWrite> {
        let key = resource_key(prefix, collection, id);
        let body = serde_json::to_vec(value).context("failed to serialize admin payload")?;
        let response = self.client.put(key.clone(), body, None).await.context("failed to write admin config")?;
        let revision = response.header().map(|h| h.revision()).unwrap_or_default();
        Ok(AdminStoreWrite { key, revision })
    }
}
```

Keep it intentionally minimal. Do not add generic repository traits.

- [ ] **Step 4: Run the focused etcd helper tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config joins_collection_paths_under_prefix normalizes_trailing_slashes_in_prefix -- --nocapture
```

Expected:
- PASS.

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-config/src/etcd.rs aisix/crates/aisix-config/Cargo.toml aisix/crates/aisix-config/src/lib.rs
git commit -m "feat(config): add etcd read write client"
```

### Task 4: Implement Watch-Driven Snapshot Reload

**Files:**
- Modify: `aisix/crates/aisix-config/src/watcher.rs`
- Create: `aisix/crates/aisix-config/tests/etcd_watch.rs`
- Modify: `aisix/crates/aisix-config/src/loader.rs`
- Modify: `aisix/crates/aisix-config/src/etcd.rs`

- [ ] **Step 1: Write the failing watcher behavior tests**

Add tests that define the required behavior:
- load current snapshot
- after a watched update, eventually replace snapshot
- if updated etcd contents no longer compile, old snapshot stays active

Target test shape:

```rust
// aisix/crates/aisix-config/tests/etcd_watch.rs
#[tokio::test]
async fn watcher_reloads_snapshot_after_put() {
    let harness = EtcdHarness::start().await;
    harness.put_json("/aisix/providers/openai", json!({
        "id": "openai",
        "kind": "openai",
        "base_url": "https://api.openai.com",
        "auth": { "secret_ref": "env:OPENAI_API_KEY" },
        "policy_id": null,
        "rate_limit": null
    })).await;

    let snapshot = aisix_config::watcher::initial_snapshot_handle(
        aisix_config::loader::compile_snapshot_from_entries("/aisix", &[], 0).unwrap()
    );

    let _task = aisix_config::watcher::spawn_snapshot_watcher(
        harness.config(),
        "/aisix".to_string(),
        snapshot.clone(),
    ).await.unwrap();

    harness.put_json("/aisix/models/gpt-4o-mini", json!({
        "id": "gpt-4o-mini",
        "provider_id": "openai",
        "upstream_model": "gpt-4o-mini",
        "policy_id": null,
        "rate_limit": null
    })).await;

    wait_until(|| snapshot.load().models_by_name.contains_key("gpt-4o-mini")).await;
}
```

And invalid-update protection:

```rust
#[tokio::test]
async fn watcher_keeps_previous_snapshot_when_reload_fails() {
    // seed valid provider/model/apikey
    // start watcher
    // write invalid model referencing missing provider
    // assert previously valid key/model remain present after retry window
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture
```

Expected:
- FAIL due to missing watcher orchestration and etcd harness helpers.

- [ ] **Step 3: Implement the watcher**

Replace the placeholder watcher with:
- `initial_snapshot_handle(snapshot)`
- `load_initial_snapshot(config) -> Result<CompiledSnapshot>`
- `spawn_snapshot_watcher(config, snapshot) -> Result<JoinHandle<()>>`

Behavior:
- connect to etcd
- establish watch on prefix
- debounce bursts with a short timer
- reload using full `load_prefix`
- compile with latest etcd revision
- `snapshot.store(...)` only on successful compile
- log/retry on watch failure
- reconnect loop on stream closure

Target core loop:

```rust
pub async fn load_initial_snapshot(config: &StartupConfig) -> anyhow::Result<CompiledSnapshot> {
    let mut store = EtcdStore::connect(&config.etcd).await?;
    let (entries, revision) = store.load_prefix(&config.etcd.prefix).await?;
    let snapshot = compile_snapshot_from_entries(&config.etcd.prefix, &entries, revision)
        .map_err(anyhow::Error::msg)?;
    Ok(snapshot)
}
```

```rust
pub async fn spawn_snapshot_watcher(
    config: StartupConfig,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    Ok(tokio::spawn(async move {
        loop {
            if let Err(error) = watch_once(&config, snapshot.clone()).await {
                tracing::warn!(error = %error, "snapshot watcher failed");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }))
}
```

Inside `watch_once`, debounce events and call a single shared `reload_snapshot(...)`.

- [ ] **Step 4: Run the watcher/config tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile etcd_loader etcd_watch -- --nocapture
```

Expected:
- PASS once live etcd harness or container-backed integration tests are wired correctly.

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-config/src/watcher.rs aisix/crates/aisix-config/tests/etcd_watch.rs aisix/crates/aisix-config/src/loader.rs aisix/crates/aisix-config/src/etcd.rs
git commit -m "feat(config): add etcd watch snapshot reload"
```

### Task 5: Bootstrap Runtime from Etcd and Start Background Watcher

**Files:**
- Modify: `aisix/crates/aisix-runtime/src/bootstrap.rs`
- Modify: `aisix/bin/aisix-gateway/src/main.rs`
- Modify: `aisix/crates/aisix-runtime/Cargo.toml`

- [ ] **Step 1: Write the failing bootstrap behavior test**

Add a runtime-level integration test or focused config/bootstrap test asserting:
- startup fails if etcd is unavailable
- startup succeeds when etcd contains valid config
- initial snapshot revision comes from etcd revision

If there is no runtime test file yet, add one later under `aisix/crates/aisix-runtime/tests/bootstrap.rs`.

Target behavior snippet:

```rust
#[tokio::test]
async fn bootstrap_fails_when_etcd_is_unreachable() {
    let config = StartupConfig {
        // ...
        etcd: EtcdConfig {
            endpoints: vec!["http://127.0.0.1:1".to_string()],
            prefix: "/aisix".to_string(),
            dial_timeout_ms: 100,
        },
        // ...
    };

    let error = aisix_runtime::bootstrap::bootstrap(&config).await.expect_err("startup should fail");
    assert!(error.to_string().contains("etcd"));
}
```

- [ ] **Step 2: Run the failing bootstrap test**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-runtime bootstrap -- --nocapture
```

Expected:
- FAIL because bootstrap still creates an empty snapshot unconditionally.

- [ ] **Step 3: Implement bootstrap around initial etcd load**

Change bootstrap so it:
- calls `aisix_config::watcher::load_initial_snapshot(config).await?`
- creates the `ArcSwap` from the loaded snapshot
- starts the watcher task before returning `AppState`
- keeps the app `ready` when bootstrap succeeds
- preserves existing Redis wiring

Target implementation shape:

```rust
pub async fn bootstrap(config: &StartupConfig) -> Result<AppState> {
    let initial = aisix_config::watcher::load_initial_snapshot(config).await?;
    let snapshot = aisix_config::watcher::initial_snapshot_handle(initial);
    let redis = RedisPool::from_url(&config.redis.url)?;
    let state = AppState::with_redis(snapshot.clone(), true, Some(redis));

    let _watcher = aisix_config::watcher::spawn_snapshot_watcher(config.clone(), snapshot).await?;

    Ok(state)
}
```

Also ensure the watcher task handle is intentionally detached or held safely without immediate drop bugs.

- [ ] **Step 4: Run runtime tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-runtime -- --nocapture
```

Expected:
- PASS, including the new bootstrap behavior coverage.

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-runtime/src/bootstrap.rs aisix/bin/aisix-gateway/src/main.rs aisix/crates/aisix-runtime/Cargo.toml
git commit -m "feat(runtime): load initial config from etcd"
```

### Task 6: Replace Admin In-Memory Store with Etcd Writes

**Files:**
- Modify: `aisix/crates/aisix-server/src/admin/mod.rs`
- Modify: `aisix/crates/aisix-server/src/admin/providers.rs`
- Modify: `aisix/crates/aisix-server/src/admin/models.rs`
- Modify: `aisix/crates/aisix-server/src/admin/apikeys.rs`
- Modify: `aisix/crates/aisix-server/src/admin/policies.rs`
- Modify: `aisix/bin/aisix-gateway/src/main.rs`

- [ ] **Step 1: Write the failing admin write tests**

Adapt `aisix/crates/aisix-server/tests/admin_reload.rs` so it no longer assumes immediate runtime mutation. Add/assert:
- Admin PUT returns 200 and an etcd-backed revision
- response path matches `/aisix/<collection>/<id>`
- a separate etcd read confirms persistence

Target response assertion:

```rust
let body = response.into_body().collect().await.unwrap().to_bytes();
let json: Value = serde_json::from_slice(&body).unwrap();
assert_eq!(json["id"], "openai");
assert_eq!(json["path"], "/aisix/providers/openai");
assert!(json["revision"].as_i64().unwrap() > 0);
```

- [ ] **Step 2: Run the failing server tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture
```

Expected:
- FAIL because `AdminState::new` still expects a snapshot handle and still mutates memory directly.

- [ ] **Step 3: Rewrite `AdminState` as an etcd writer wrapper**

Replace the current `AdminStore`/`StoreInner`/`compile_from_entries` flow with:
- stored admin keys
- stored etcd prefix
- stored etcd connection config or write handle

Target shape:

```rust
#[derive(Debug, Clone)]
pub struct AdminState {
    keys: Arc<HashSet<String>>,
    prefix: String,
    etcd: Arc<tokio::sync::Mutex<aisix_config::etcd::EtcdStore>>,
}

impl AdminState {
    pub async fn from_startup_config(config: &StartupConfig) -> anyhow::Result<Option<Self>> {
        if !config.deployment.admin.enabled {
            return Ok(None);
        }

        let etcd = aisix_config::etcd::EtcdStore::connect(&config.etcd).await?;
        Ok(Some(Self {
            keys: Arc::new(config.deployment.admin.admin_keys.iter().map(|k| k.key.clone()).collect()),
            prefix: config.etcd.prefix.clone(),
            etcd: Arc::new(tokio::sync::Mutex::new(etcd)),
        }))
    }

    pub async fn put_provider(&self, id: &str, provider: ProviderConfig) -> Result<AdminWriteResult, GatewayError> {
        self.put("providers", id, &provider).await
    }
}
```

And shared put logic:

```rust
async fn put<T: Serialize>(&self, collection: &str, id: &str, value: &T) -> Result<AdminWriteResult, GatewayError> {
    let mut etcd = self.etcd.lock().await;
    let write = etcd
        .put_json(&self.prefix, collection, id, value)
        .await
        .map_err(internal_admin_error)?;

    Ok(AdminWriteResult {
        id: id.to_string(),
        path: write.key,
        revision: write.revision,
    })
}
```

Delete the old in-memory compilation code entirely.

- [ ] **Step 4: Update the HTTP handlers and gateway startup**

Update each admin handler to await the new async admin methods:

```rust
let result = admin.put_provider(&id, provider).await?;
```

Update gateway startup:

```rust
let admin = aisix_server::admin::AdminState::from_startup_config(&config).await?;
```

- [ ] **Step 5: Run the focused admin/auth tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_requires_valid_x_admin_key admin_rejects_path_and_body_id_mismatch -- --nocapture
```

Expected:
- PASS.
- Broader reload tests may still need the watcher integration update from the next task.

- [ ] **Step 6: Commit**

```bash
git add aisix/crates/aisix-server/src/admin/mod.rs aisix/crates/aisix-server/src/admin/providers.rs aisix/crates/aisix-server/src/admin/models.rs aisix/crates/aisix-server/src/admin/apikeys.rs aisix/crates/aisix-server/src/admin/policies.rs aisix/bin/aisix-gateway/src/main.rs
git commit -m "feat(admin): write config changes to etcd"
```

### Task 7: Convert Admin Reload Tests to Eventual-Consistency Watch Semantics

**Files:**
- Modify: `aisix/crates/aisix-server/tests/admin_reload.rs`
- Possibly create: `aisix/crates/aisix-server/tests/support/etcd.rs` if reuse justifies it

- [ ] **Step 1: Rewrite the failing integration tests around watch-based activation**

Replace direct in-memory `test_state(...)` construction with a live etcd-backed app bootstrap path. Add a polling helper that waits until a request starts succeeding or until a timeout expires.

Target helper:

```rust
async fn wait_until_ready<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("condition not met before timeout");
}
```

Then express the primary behavior as:
- PUT provider/model/apikey via Admin API
- poll until chat request returns `200`
- assert provider header and revision behavior

And the invalid update behavior as:
- seed valid runtime
- trigger an invalid model write to etcd through Admin
- poll briefly
- assert old runtime config still serves requests and limit behavior remains intact

- [ ] **Step 2: Run the failing integration tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture
```

Expected:
- FAIL until tests are fully migrated off the old in-memory assumptions and onto real etcd-backed bootstrap.

- [ ] **Step 3: Implement the live etcd-backed test harness**

The harness should:
- start from empty etcd prefix
- build `StartupConfig` with admin enabled and test etcd endpoint
- call real `aisix_runtime::bootstrap`
- call real `AdminState::from_startup_config`
- construct the router through `aisix_server::app::build_router`

Keep helper code local unless it is reused by multiple test files.

- [ ] **Step 4: Run the full admin reload test file**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture
```

Expected:
- PASS, including:
  - admin writes persist and later become active
  - invalid updates do not poison the active snapshot
  - auth and id-mismatch validation still hold

- [ ] **Step 5: Commit**

```bash
git add aisix/crates/aisix-server/tests/admin_reload.rs
git commit -m "test(admin): cover etcd-backed reload flow"
```

### Task 8: Update Startup Docs and Smoke Flow for Etcd-Backed Semantics

**Files:**
- Modify: `aisix/README.md`
- Modify: `aisix/config/aisix-gateway.example.yaml` only if comments/examples need clarification without changing behavior

- [ ] **Step 1: Write the failing doc expectation**

Define the acceptance check manually:
- README must stop implying Admin writes mutate in-memory runtime immediately.
- README must explain that Admin writes go to etcd and become active after watch reload.

- [ ] **Step 2: Update the README**

Revise the getting-started flow to state:
- `docker compose up -d redis etcd`
- gateway startup requires reachable etcd
- Admin `PUT` stores config in etcd
- runtime activation is asynchronous via watcher
- if an invalid config is written, Admin may still succeed but runtime keeps the previous snapshot

Target wording block:

```md
The embedded Admin API writes config into etcd under the configured prefix. Runtime changes are applied asynchronously by the background etcd watcher after the new full snapshot compiles successfully. A successful Admin response means etcd accepted the write; it does not guarantee the new config is already active.
```

- [ ] **Step 3: Run the relevant tests/checks**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config startup_config -- --nocapture
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture
```

Expected:
- PASS, with docs aligned to tested behavior.

- [ ] **Step 4: Commit**

```bash
git add aisix/README.md aisix/config/aisix-gateway.example.yaml
git commit -m "docs: clarify etcd-backed admin reload behavior"
```

### Task 9: Final Verification

**Files:**
- No new files expected
- Re-run tests across touched crates

- [ ] **Step 1: Run config crate tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-config -- --nocapture
```

Expected:
- PASS.

- [ ] **Step 2: Run runtime crate tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-runtime -- --nocapture
```

Expected:
- PASS.

- [ ] **Step 3: Run server crate tests**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server -- --nocapture
```

Expected:
- PASS.

- [ ] **Step 4: Run an end-to-end smoke check**

Run:

```bash
cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_can_create_provider_model_and_apikey_then_gateway_uses_reloaded_snapshot -- --nocapture
```

Expected:
- PASS.

- [ ] **Step 5: Final commit if verification required a fix**

```bash
git add <any-fixed-files>
git commit -m "fix: address etcd admin reload verification issues"
```

## Self-review

- Spec coverage: covered Admin only writing etcd, startup failing when etcd is unavailable, watch-driven full reload, compile-fail preserving old snapshot, and doc/test updates for eventual consistency.
- Placeholder scan: no `TODO`/`TBD`/“handle appropriately” placeholders remain; commands, files, and code shapes are explicit.
- Type consistency: plan consistently uses `EtcdStore`, `EtcdEntry`, `AdminWriteResult.revision`, `CompiledSnapshot.revision`, and `load_initial_snapshot` / `spawn_snapshot_watcher`.

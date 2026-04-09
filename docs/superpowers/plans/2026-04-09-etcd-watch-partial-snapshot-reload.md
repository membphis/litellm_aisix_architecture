# Etcd Watch Partial Snapshot Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make etcd-backed snapshot reloads continue applying valid config when some resources are invalid, while treating invalid current resources as absent from the runtime snapshot.

**Architecture:** Keep the current immutable `CompiledSnapshot` + `ArcSwap` model, but change compilation from fail-fast-on-reference-errors to a report-based pipeline that emits a valid snapshot plus skipped-resource warnings. Loader parsing becomes tolerant for per-entry decode/id mismatches, compile filters invalid resources by dependency order, and the watcher only advances its in-memory revision after a new snapshot has been successfully stored.

**Tech Stack:** Rust, Tokio, `arc-swap`, `tracing`, etcd, workspace crates (`aisix-config`, `aisix-core`, `aisix-runtime`)

---

## File Map

### Existing files to modify

- `aisix/crates/aisix-config/src/compile.rs`
  - Replace fail-fast reference validation with report-based partial compilation.
- `aisix/crates/aisix-config/src/loader.rs`
  - Tolerate per-entry decode and key/body-id mismatches, while preserving hard errors for malformed keys and unsupported collections.
- `aisix/crates/aisix-config/src/watcher.rs`
  - Consume the compile report, log skipped resources, and only advance watcher revision after a compiled snapshot is stored.
- `aisix/crates/aisix-config/src/lib.rs`
  - Re-export new compile/loader report types if needed by tests or callers.
- `aisix/crates/aisix-config/tests/snapshot_compile.rs`
  - Replace expectations that unknown references hard-fail with expectations that invalid resources are skipped.
- `aisix/crates/aisix-config/tests/etcd_loader.rs`
  - Replace loader decode/id mismatch hard-failure tests with partial-compile assertions.
- `aisix/crates/aisix-config/tests/etcd_watch.rs`
  - Add coverage for continued snapshot progress while invalid resources remain in etcd.
- `aisix/docs/admin-api.md`
  - Document that runtime applies the valid subset of etcd config and drops invalid current resources until fixed.

### Existing files to reuse without planned modification

- `aisix/crates/aisix-config/src/snapshot.rs`
  - `CompiledSnapshot` shape remains the runtime contract.
- `aisix/crates/aisix-runtime/src/bootstrap.rs`
  - Startup should continue to call `load_initial_snapshot`, now receiving a partially compiled snapshot.
- `aisix/crates/aisix-core/src/app_state.rs`
  - Snapshot storage remains `Arc<ArcSwap<CompiledSnapshot>>`.

### Test commands to use during execution

- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config`
- `cargo test --manifest-path aisix/Cargo.toml`
- `cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings`

## Task 1: Introduce Snapshot Compile Reports

**Files:**
- Modify: `aisix/crates/aisix-config/src/compile.rs`
- Modify: `aisix/crates/aisix-config/src/lib.rs`
- Test: `aisix/crates/aisix-config/tests/snapshot_compile.rs`

- [ ] **Step 1: Write the failing compile-report test**

Append this test near the current invalid-reference tests in `aisix/crates/aisix-config/tests/snapshot_compile.rs`:

```rust
#[test]
fn compile_snapshot_skips_models_with_missing_provider_references() {
    let mut broken_model = model();
    broken_model.id = "broken-model".to_string();
    broken_model.provider_id = "missing-provider".to_string();

    let report = compile_snapshot(
        vec![provider()],
        vec![model(), broken_model],
        vec![],
        vec![policy()],
        11,
    )
    .expect("snapshot should compile by skipping invalid models");

    assert!(report.snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(!report.snapshot.models_by_name.contains_key("broken-model"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "model");
    assert_eq!(report.issues[0].id, "broken-model");
    assert_eq!(
        report.issues[0].reason,
        "missing provider reference: missing-provider"
    );
}
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config compile_snapshot_skips_models_with_missing_provider_references -- --nocapture`
Expected: FAIL because `compile_snapshot(...)` still returns `CompiledSnapshot` directly and still errors on unknown provider references.

- [ ] **Step 3: Add report types and partial-compilation flow in `compile.rs`**

Replace the top-level compile API in `aisix/crates/aisix-config/src/compile.rs` with code shaped like this:

```rust
use std::collections::{HashMap, HashSet};

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig};
use crate::snapshot::{CompiledSnapshot, ResolvedLimits};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileIssue {
    pub kind: &'static str,
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCompileReport {
    pub snapshot: CompiledSnapshot,
    pub issues: Vec<CompileIssue>,
}

pub fn compile_snapshot(
    providers: Vec<ProviderConfig>,
    models: Vec<ModelConfig>,
    apikeys: Vec<ApiKeyConfig>,
    policies: Vec<PolicyConfig>,
    revision: i64,
) -> Result<SnapshotCompileReport, String> {
    let policies_by_id = collect_unique_by_id(policies, "policy")?;
    let mut issues = Vec::new();

    let mut providers_by_id = HashMap::new();
    let mut provider_limits = HashMap::new();
    for provider in collect_unique_by_id(providers, "provider")?.into_values() {
        match validate_policy_reference(provider.policy_id.as_deref(), &policies_by_id)
            .and_then(|_| {
                resolve_limits(
                    provider.rate_limit.as_ref(),
                    provider.policy_id.as_deref(),
                    &policies_by_id,
                )
            }) {
            Ok(limits) => {
                provider_limits.insert(provider.id.clone(), limits);
                providers_by_id.insert(provider.id.clone(), provider);
            }
            Err(reason) => issues.push(CompileIssue {
                kind: "provider",
                id: provider.id,
                reason,
            }),
        }
    }

    let mut models_by_name = HashMap::new();
    let mut model_limits = HashMap::new();
    for model in collect_unique_by_id(models, "model")?.into_values() {
        let result = if !providers_by_id.contains_key(&model.provider_id) {
            Err(format!("missing provider reference: {}", model.provider_id))
        } else {
            validate_policy_reference(model.policy_id.as_deref(), &policies_by_id).and_then(|_| {
                resolve_limits(
                    model.rate_limit.as_ref(),
                    model.policy_id.as_deref(),
                    &policies_by_id,
                )
            })
        };

        match result {
            Ok(limits) => {
                model_limits.insert(model.id.clone(), limits);
                models_by_name.insert(model.id.clone(), model);
            }
            Err(reason) => issues.push(CompileIssue {
                kind: "model",
                id: model.id,
                reason,
            }),
        }
    }

    let apikeys_by_id = collect_unique_by_id(apikeys, "api key")?;
    let mut retained_apikeys = HashMap::new();
    let mut keys_by_token = HashMap::new();
    let mut key_limits = HashMap::new();
    let mut seen_tokens = HashSet::new();

    for api_key in apikeys_by_id.into_values() {
        let missing_model = api_key
            .allowed_models
            .iter()
            .find(|model_name| !models_by_name.contains_key(*model_name));

        let result = if let Some(model_name) = missing_model {
            Err(format!("missing model reference: {model_name}"))
        } else {
            validate_policy_reference(api_key.policy_id.as_deref(), &policies_by_id).and_then(|_| {
                resolve_limits(
                    api_key.rate_limit.as_ref(),
                    api_key.policy_id.as_deref(),
                    &policies_by_id,
                )
            })
        };

        match result {
            Ok(limits) => {
                if !seen_tokens.insert(api_key.key.clone()) {
                    return Err("duplicate api key token".to_string());
                }

                key_limits.insert(api_key.id.clone(), limits);
                keys_by_token.insert(
                    api_key.key.clone(),
                    KeyMeta {
                        key_id: api_key.id.clone(),
                        user_id: None,
                        customer_id: None,
                        alias: None,
                        expires_at: None,
                        allowed_models: api_key.allowed_models.clone(),
                    },
                );
                retained_apikeys.insert(api_key.id.clone(), api_key);
            }
            Err(reason) => issues.push(CompileIssue {
                kind: "api key",
                id: api_key.id,
                reason,
            }),
        }
    }

    Ok(SnapshotCompileReport {
        snapshot: CompiledSnapshot {
            revision,
            keys_by_token,
            apikeys_by_id: retained_apikeys,
            providers_by_id,
            models_by_name,
            policies_by_id,
            provider_limits,
            model_limits,
            key_limits,
        },
        issues,
    })
}
```

Also re-export the new types from `aisix/crates/aisix-config/src/lib.rs`:

```rust
pub mod compile;
pub use compile::{CompileIssue, SnapshotCompileReport};
```

- [ ] **Step 4: Update the existing duplicate-token test to keep hard-error semantics**

Keep the existing hard-error assertion in `aisix/crates/aisix-config/tests/snapshot_compile.rs` aligned with the new return type:

```rust
let error = compile_snapshot(
    vec![provider()],
    vec![model()],
    vec![api_key(), second_key],
    vec![policy()],
    1,
)
.expect_err("duplicate plaintext keys should be rejected");

assert_eq!(error, "duplicate api key token");
```

- [ ] **Step 5: Run the compile test set to verify it passes**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture`
Expected: PASS, with tests asserting that reference errors now become skipped-resource issues while duplicate IDs and duplicate key tokens still hard-fail.

- [ ] **Step 6: Commit**

```bash
git add aisix/crates/aisix-config/src/compile.rs aisix/crates/aisix-config/src/lib.rs aisix/crates/aisix-config/tests/snapshot_compile.rs
git commit -m "feat: compile partial snapshots from valid config"
```

## Task 2: Make Loader Parsing Tolerant Per Entry

**Files:**
- Modify: `aisix/crates/aisix-config/src/loader.rs`
- Modify: `aisix/crates/aisix-config/src/lib.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_loader.rs`

- [ ] **Step 1: Write the failing loader test for decode mismatch tolerance**

Replace the current mismatch-fails test in `aisix/crates/aisix-config/tests/etcd_loader.rs` with this test:

```rust
#[test]
fn skips_key_and_payload_id_mismatches() {
    let entries = vec![
        EtcdEntry::json(
            "/aisix/providers/openai",
            &json!({
                "id": "anthropic",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        ),
        EtcdEntry::json(
            "/aisix/providers/anthropic",
            &json!({
                "id": "anthropic",
                "kind": "anthropic",
                "base_url": "https://api.anthropic.com",
                "auth": { "secret_ref": "env:ANTHROPIC_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        ),
    ];

    let report = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect("mismatched entry should be skipped, not fail");

    assert!(report.snapshot.providers_by_id.contains_key("anthropic"));
    assert!(!report.snapshot.providers_by_id.contains_key("openai"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "provider");
    assert_eq!(report.issues[0].id, "openai");
    assert!(report.issues[0]
        .reason
        .contains("etcd key/body id mismatch for provider"));
}
```

- [ ] **Step 2: Run the targeted loader test to verify it fails**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config skips_key_and_payload_id_mismatches -- --nocapture`
Expected: FAIL because `compile_snapshot_from_entries(...)` still returns `CompiledSnapshot` directly and still hard-fails on mismatch.

- [ ] **Step 3: Introduce loader-side entry issue collection**

Rewrite the loader entry parsing in `aisix/crates/aisix-config/src/loader.rs` like this:

```rust
use serde::Serialize;

use crate::{
    compile::{CompileIssue, SnapshotCompileReport, compile_snapshot},
    etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig},
};

pub fn compile_snapshot_from_entries(
    prefix: &str,
    entries: &[EtcdEntry],
    revision: i64,
) -> Result<SnapshotCompileReport, String> {
    let normalized_prefix = format!("{}/", prefix.trim_end_matches('/'));
    let mut providers = Vec::new();
    let mut models = Vec::new();
    let mut apikeys = Vec::new();
    let mut policies = Vec::new();
    let mut issues = Vec::new();

    for entry in entries {
        let relative = entry
            .key
            .strip_prefix(&normalized_prefix)
            .ok_or_else(|| format!("invalid etcd key outside prefix: {}", entry.key))?;

        let mut parts = relative.split('/');
        let collection = parts
            .next()
            .filter(|segment| !segment.is_empty())
            .ok_or_else(|| format!("invalid etcd key: {}", entry.key))?;
        let resource_id = parts
            .next()
            .filter(|segment| !segment.is_empty())
            .ok_or_else(|| format!("invalid etcd key: {}", entry.key))?;

        if parts.next().is_some() || resource_id.is_empty() {
            return Err(format!("invalid etcd key: {}", entry.key));
        }

        match collection {
            "providers" => push_decoded::<ProviderConfig>(
                entry,
                "provider",
                resource_id,
                &mut providers,
                &mut issues,
            ),
            "models" => push_decoded::<ModelConfig>(
                entry,
                "model",
                resource_id,
                &mut models,
                &mut issues,
            ),
            "apikeys" => push_decoded::<ApiKeyConfig>(
                entry,
                "api key",
                resource_id,
                &mut apikeys,
                &mut issues,
            ),
            "policies" => push_decoded::<PolicyConfig>(
                entry,
                "policy",
                resource_id,
                &mut policies,
                &mut issues,
            ),
            other => return Err(format!("unsupported etcd collection: {other}")),
        }
    }

    let mut report = compile_snapshot(providers, models, apikeys, policies, revision)?;
    issues.append(&mut report.issues);
    report.issues = issues;
    Ok(report)
}

fn push_decoded<T>(
    entry: &EtcdEntry,
    kind: &'static str,
    resource_id: &str,
    out: &mut Vec<T>,
    issues: &mut Vec<CompileIssue>,
) where
    T: serde::de::DeserializeOwned + HasConfigId,
{
    match decode_entry::<T>(entry, kind, resource_id) {
        Ok(decoded) => out.push(decoded),
        Err(reason) => issues.push(CompileIssue {
            kind,
            id: resource_id.to_string(),
            reason,
        }),
    }
}
```

Keep `decode_entry(...)` returning `Result<T, String>` so malformed JSON and key/body-id mismatches become loader issues instead of hard failures.

- [ ] **Step 4: Update the existing decode-error test to assert skip behavior**

Replace the current decode-error assertion with:

```rust
#[test]
fn includes_key_and_type_in_decode_issues() {
    let entries = vec![EtcdEntry {
        key: "/aisix/models/gpt-4o-mini".to_string(),
        value: br#"{"id":"gpt-4o-mini","provider_id":123}"#.to_vec(),
    }];

    let report = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect("decode failures should be reported as skipped resources");

    assert!(report.snapshot.models_by_name.is_empty());
    assert_eq!(report.issues.len(), 1);
    assert!(report.issues[0]
        .reason
        .starts_with("failed to decode model at /aisix/models/gpt-4o-mini: "));
}
```

- [ ] **Step 5: Run the loader test set to verify it passes**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader -- --nocapture`
Expected: PASS, with malformed keys and unsupported collections still failing hard, but single-entry decode/id mismatches compiling into issue reports.

- [ ] **Step 6: Commit**

```bash
git add aisix/crates/aisix-config/src/loader.rs aisix/crates/aisix-config/src/lib.rs aisix/crates/aisix-config/tests/etcd_loader.rs
git commit -m "feat: skip invalid etcd entries during snapshot load"
```

## Task 3: Update Watcher Revision Semantics And Logging

**Files:**
- Modify: `aisix/crates/aisix-config/src/watcher.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_watch.rs`

- [ ] **Step 1: Add the failing watcher regression test**

Append this test to `aisix/crates/aisix-config/tests/etcd_watch.rs`:

```rust
#[tokio::test]
async fn watcher_applies_unrelated_valid_updates_while_invalid_model_is_skipped() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("provider fixture should be written");

    let snapshot = initial_snapshot_handle(
        aisix_config::watcher::load_initial_snapshot(&aisix_config::startup::StartupConfig {
            server: aisix_config::startup::ServerConfig {
                listen: "127.0.0.1:0".to_string(),
                metrics_listen: "127.0.0.1:0".to_string(),
                request_body_limit_mb: 1,
            },
            etcd: harness.config(),
            redis: aisix_config::startup::RedisConfig {
                url: "redis://127.0.0.1:1".to_string(),
            },
            log: aisix_config::startup::LogConfig {
                level: "info".to_string(),
            },
            runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
            deployment: aisix_config::startup::DeploymentConfig {
                admin: aisix_config::startup::AdminConfig {
                    enabled: false,
                    admin_keys: vec![],
                },
            },
        })
        .await
        .expect("initial snapshot should load"),
    );
    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

    harness
        .put_json(
            "/aisix/models/broken-model",
            &json!({
                "id": "broken-model",
                "provider_id": "missing-provider",
                "upstream_model": "broken-model",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("broken model fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "openai",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("valid model fixture should be written");

    wait_until(|| snapshot.load().models_by_name.contains_key("gpt-4o-mini")).await;

    assert!(!snapshot.load().models_by_name.contains_key("broken-model"));
    watcher.abort();
}
```

- [ ] **Step 2: Run the targeted watcher test to verify it fails**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config watcher_applies_unrelated_valid_updates_while_invalid_model_is_skipped -- --nocapture`
Expected: FAIL because the watcher still reloads the whole prefix with fail-fast semantics and therefore never publishes the valid model while the broken model exists.

- [ ] **Step 3: Move watcher revision storage after successful snapshot publication**

Update `reload_snapshot_and_revision(...)` in `aisix/crates/aisix-config/src/watcher.rs` to this shape:

```rust
async fn reload_snapshot_and_revision(
    config: &EtcdConfig,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    revision: Arc<AtomicI64>,
) -> Result<()> {
    let mut store = EtcdStore::connect(config).await?;
    let (entries, new_revision) = store.load_prefix(&config.prefix).await?;

    let report = compile_snapshot_from_entries(&config.prefix, &entries, new_revision)
        .map_err(anyhow::Error::msg)?;

    log_snapshot_reload(new_revision, &report.issues);

    snapshot.store(Arc::new(report.snapshot));
    revision.store(new_revision, Ordering::Release);
    Ok(())
}
```

Add these helpers in the same file:

```rust
fn log_snapshot_reload(revision: i64, issues: &[crate::compile::CompileIssue]) {
    if issues.is_empty() {
        tracing::info!(revision, "snapshot reloaded");
        return;
    }

    tracing::info!(revision, skipped_resources = issues.len(), "snapshot reloaded with skipped resources");
    for issue in issues {
        tracing::warn!(
            revision,
            resource_kind = issue.kind,
            resource_id = %issue.id,
            reason = %issue.reason,
            "skipping invalid config resource"
        );
    }
}
```

Also update `reload_snapshot(...)` to return `report.snapshot` from the same report-based loader API:

```rust
async fn reload_snapshot(store: &mut EtcdStore, config: &EtcdConfig) -> Result<CompiledSnapshot> {
    let (entries, revision) = store.load_prefix(&config.prefix).await?;
    let report = compile_snapshot_from_entries(&config.prefix, &entries, revision)
        .map_err(anyhow::Error::msg)?;
    log_snapshot_reload(revision, &report.issues);
    Ok(report.snapshot)
}
```

- [ ] **Step 4: Add a targeted test for revision progression after valid reloads**

Append this test to `aisix/crates/aisix-config/tests/etcd_watch.rs`:

```rust
#[tokio::test]
async fn watcher_revision_advances_after_partial_reload_success() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("provider fixture should be written");

    let initial = aisix_config::watcher::load_initial_snapshot(&aisix_config::startup::StartupConfig {
        server: aisix_config::startup::ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: aisix_config::startup::RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: aisix_config::startup::LogConfig {
            level: "info".to_string(),
        },
        runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
        deployment: aisix_config::startup::DeploymentConfig {
            admin: aisix_config::startup::AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    })
    .await
    .expect("initial snapshot should load");
    let initial_revision = initial.revision;
    let snapshot = initial_snapshot_handle(initial);
    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

    let later_revision = harness
        .put_json(
            "/aisix/models/broken-model",
            &json!({
                "id": "broken-model",
                "provider_id": "missing-provider",
                "upstream_model": "broken-model",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("broken model fixture should be written");

    wait_until(|| snapshot.load().revision >= later_revision).await;
    assert!(snapshot.load().revision > initial_revision);
    watcher.abort();
}
```

- [ ] **Step 5: Run the watcher test set to verify it passes**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture`
Expected: PASS, with the watcher keeping broken resources out of the snapshot while still publishing unrelated valid changes and advancing to the latest successfully published revision.

- [ ] **Step 6: Commit**

```bash
git add aisix/crates/aisix-config/src/watcher.rs aisix/crates/aisix-config/tests/etcd_watch.rs
git commit -m "fix: keep snapshot reloads progressing with invalid resources"
```

## Task 4: Document Runtime Semantics And Run Full Verification

**Files:**
- Modify: `aisix/docs/admin-api.md`
- Test: `aisix/crates/aisix-config/tests/etcd_loader.rs`
- Test: `aisix/crates/aisix-config/tests/snapshot_compile.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_watch.rs`

- [ ] **Step 1: Update the admin API semantics section**

Edit `aisix/docs/admin-api.md` so the overview explicitly says this:

```markdown
Admin writes are accepted by etcd first and applied to the live gateway later by the background watcher.
This means a successful `PUT` or `DELETE` response confirms the config change was stored in etcd, not that the new runtime snapshot is already active.

The runtime snapshot is compiled from the valid subset of config currently stored under the prefix.
If a resource is invalid (for example, a model references a missing provider), that resource is skipped and treated as absent from the live snapshot until it is fixed.
Other valid resources continue to reload normally.
```

Also update the example section for writing a model before its provider exists to this:

```markdown
The write can succeed even if `openai` has not been written yet.
Until the referenced provider exists, that model is treated as invalid and is absent from the runtime snapshot.
Other valid resources continue to apply.
Once the provider is written, a later reload includes the model automatically.
```

- [ ] **Step 2: Run crate-level verification**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config`
Expected: PASS

- [ ] **Step 3: Run workspace-level verification**

Run: `cargo test --manifest-path aisix/Cargo.toml`
Expected: PASS

- [ ] **Step 4: Run lint verification**

Run: `cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add aisix/docs/admin-api.md
git commit -m "docs: clarify partial snapshot runtime semantics"
```

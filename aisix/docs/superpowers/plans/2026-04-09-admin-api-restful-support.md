# Admin API RESTful Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the Admin API so `providers`, `models`, `apikeys`, and `policies` support collection `GET` plus item `GET`/`PUT`/`DELETE`, with deterministic list ordering and `404` for missing resources.

**Architecture:** Keep the existing explicit per-resource Axum handlers in `crates/aisix-server/src/admin/`, add shared read/delete helpers in `AdminState`, and extend the etcd store with typed `get_json` and `list_json` helpers plus delete metadata. Expose `404` through a new `ErrorKind::NotFound`, then cover the new behavior in live-etcd integration tests.

**Tech Stack:** Rust, Axum, serde, etcd-client, Tokio, cargo integration tests

---

## File Structure

- Modify: `crates/aisix-types/src/error.rs`
  Add `ErrorKind::NotFound` and map it to `404 Not Found`.
- Modify: `crates/aisix-config/src/etcd.rs`
  Add typed `get_json` / `list_json` helpers and make delete report whether a key existed.
- Create: `crates/aisix-config/tests/admin_store.rs`
  Verify the etcd store helpers return missing/resource states correctly.
- Modify: `crates/aisix-server/src/app.rs`
  Register collection `GET` routes and item `GET`/`PUT`/`DELETE` routes.
- Modify: `crates/aisix-server/src/admin/mod.rs`
  Add shared `get`, `list`, `delete`, and not-found helpers plus typed wrappers per resource.
- Modify: `crates/aisix-server/src/admin/providers.rs`
  Add provider collection/item `GET` and item `DELETE` handlers.
- Modify: `crates/aisix-server/src/admin/models.rs`
  Add model collection/item `GET` and item `DELETE` handlers.
- Modify: `crates/aisix-server/src/admin/apikeys.rs`
  Add apikey collection/item `GET` and item `DELETE` handlers.
- Modify: `crates/aisix-server/src/admin/policies.rs`
  Add policy collection/item `GET` and item `DELETE` handlers.
- Modify: `crates/aisix-server/tests/admin_reload.rs`
  Add live-etcd integration coverage for collection reads, item reads, deletes, auth failures, missing-resource `404`, and deterministic ordering.

### Task 1: Extend Error and Etcd Store Primitives

**Files:**
- Modify: `crates/aisix-types/src/error.rs`
- Modify: `crates/aisix-config/src/etcd.rs`
- Create: `crates/aisix-config/tests/admin_store.rs`

- [ ] **Step 1: Write the failing store tests**

Create `crates/aisix-config/tests/admin_store.rs` with live-etcd coverage for typed reads and delete metadata:

```rust
use aisix_config::{
    etcd::{EtcdStore, resource_key},
    etcd_model::{ProviderAuth, ProviderConfig, ProviderKind},
};

mod support {
    pub mod etcd {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/etcd.rs"));
    }
}

#[tokio::test]
async fn store_can_get_and_list_admin_resources() {
    let harness = support::etcd::EtcdHarness::start().await.unwrap();
    let mut store = EtcdStore::connect(&harness.config()).await.unwrap();

    harness
        .put_json(
            &resource_key("/aisix", "providers", "openai"),
            &ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://api.openai.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
            },
        )
        .await
        .unwrap();

    let provider = store
        .get_json::<ProviderConfig>("/aisix", "providers", "openai")
        .await
        .unwrap()
        .expect("provider should exist");
    assert_eq!(provider.id, "openai");

    let listed = store
        .list_json::<ProviderConfig>("/aisix", "providers")
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "openai");
}

#[tokio::test]
async fn store_reports_missing_resources_for_get_and_delete() {
    let harness = support::etcd::EtcdHarness::start().await.unwrap();
    let mut store = EtcdStore::connect(&harness.config()).await.unwrap();

    let missing = store
        .get_json::<ProviderConfig>("/aisix", "providers", "missing")
        .await
        .unwrap();
    assert!(missing.is_none());

    let deleted = store.delete("/aisix", "providers", "missing").await.unwrap();
    assert!(!deleted.deleted);
    assert_eq!(deleted.key, "/aisix/providers/missing");
}
```

- [ ] **Step 2: Run the new config test and verify it fails**

Run: `cargo test -p aisix-config --test admin_store -v`

Expected: FAIL because `EtcdStore` does not yet expose `get_json`, `list_json`, or a delete result with a `deleted` field.

- [ ] **Step 3: Implement the minimal error and store changes**

Update `crates/aisix-types/src/error.rs` to introduce a `NotFound` variant and map it to `404`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Permission,
    InvalidRequest,
    NotFound,
    RateLimited,
    Timeout,
    Upstream,
    Internal,
}

pub fn status_code(&self) -> StatusCode {
    match self.kind {
        ErrorKind::Authentication => StatusCode::UNAUTHORIZED,
        ErrorKind::Permission => StatusCode::FORBIDDEN,
        ErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ErrorKind::Upstream => StatusCode::BAD_GATEWAY,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
```

Update `crates/aisix-config/src/etcd.rs` to add typed reads and richer delete metadata:

```rust
use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminStoreDelete {
    pub key: String,
    pub revision: i64,
    pub deleted: bool,
}

pub async fn get_json<T: DeserializeOwned>(
    &mut self,
    prefix: &str,
    collection: &str,
    id: &str,
) -> Result<Option<T>> {
    let key = resource_key(prefix, collection, id);
    let response = self
        .client
        .get(key, None)
        .await
        .context("failed to load admin config")?;

    let Some(kv) = response.kvs().first() else {
        return Ok(None);
    };

    let value = serde_json::from_slice(kv.value()).context("failed to decode admin config")?;
    Ok(Some(value))
}

pub async fn list_json<T: DeserializeOwned>(
    &mut self,
    prefix: &str,
    collection: &str,
) -> Result<Vec<T>> {
    let prefix = format!("{}/{collection}/", prefix.trim_end_matches('/'));
    let response = self
        .client
        .get(prefix, Some(etcd_client::GetOptions::new().with_prefix()))
        .await
        .context("failed to list admin config")?;

    response
        .kvs()
        .iter()
        .map(|kv| serde_json::from_slice(kv.value()).context("failed to decode admin config"))
        .collect()
}

pub async fn delete(
    &mut self,
    prefix: &str,
    collection: &str,
    id: &str,
) -> Result<AdminStoreDelete> {
    let key = resource_key(prefix, collection, id);
    let response = self
        .client
        .delete(key.clone(), None)
        .await
        .context("failed to delete admin config")?;

    Ok(AdminStoreDelete {
        key,
        revision: response.header().map(|header| header.revision()).unwrap_or(0),
        deleted: response.deleted() > 0,
    })
}
```

- [ ] **Step 4: Run the config test again and verify it passes**

Run: `cargo test -p aisix-config --test admin_store -v`

Expected: PASS for `store_can_get_and_list_admin_resources` and `store_reports_missing_resources_for_get_and_delete`.

- [ ] **Step 5: Commit the foundation changes**

```bash
git add crates/aisix-types/src/error.rs crates/aisix-config/src/etcd.rs crates/aisix-config/tests/admin_store.rs
git commit -m "feat: add admin store read and delete primitives"
```

### Task 2: Add Provider and Model REST Endpoints

**Files:**
- Modify: `crates/aisix-server/src/app.rs`
- Modify: `crates/aisix-server/src/admin/mod.rs`
- Modify: `crates/aisix-server/src/admin/providers.rs`
- Modify: `crates/aisix-server/src/admin/models.rs`
- Modify: `crates/aisix-server/tests/admin_reload.rs`

- [ ] **Step 1: Write the failing server tests for provider/model reads and deletes**

Add these tests and request helpers to `crates/aisix-server/tests/admin_reload.rs`:

```rust
#[tokio::test]
async fn admin_can_get_provider_and_model_by_id() {
    let upstream = spawn_openai_mock().await;
    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, None).await;
        let app = fixture.router();

        let provider = app
            .clone()
            .oneshot(admin_get_request("/admin/providers/openai"))
            .await
            .unwrap();
        assert_eq!(provider.status(), StatusCode::OK);

        let model = app
            .oneshot(admin_get_request("/admin/models/gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(model.status(), StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn admin_can_list_providers_and_models_in_id_order() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    app.clone()
        .oneshot(admin_put_request(
            "/admin/providers/z-openai",
            json!({
                "id": "z-openai",
                "kind": "openai",
                "base_url": "https://z.example.com",
                "auth": {"secret_ref": "env:OPENAI_API_KEY"}
            }),
        ))
        .await
        .unwrap();
    app.clone()
        .oneshot(admin_put_request(
            "/admin/providers/a-openai",
            json!({
                "id": "a-openai",
                "kind": "openai",
                "base_url": "https://a.example.com",
                "auth": {"secret_ref": "env:OPENAI_API_KEY"}
            }),
        ))
        .await
        .unwrap();

    let response = app
        .oneshot(admin_get_request("/admin/providers"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_can_delete_provider_and_model() {
    let upstream = spawn_openai_mock().await;
    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, None).await;
        let app = fixture.router();

        let delete_provider = app
            .clone()
            .oneshot(admin_delete_request("/admin/providers/openai"))
            .await
            .unwrap();
        assert_eq!(delete_provider.status(), StatusCode::OK);

        let delete_model = app
            .oneshot(admin_delete_request("/admin/models/gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(delete_model.status(), StatusCode::OK);
    })
    .await;
}

fn admin_get_request(path: &str) -> Request<Body> {
    admin_request("GET", path, None)
}

fn admin_delete_request(path: &str) -> Request<Body> {
    admin_request("DELETE", path, None)
}
```

- [ ] **Step 2: Run the targeted server tests and verify they fail**

Run: `cargo test -p aisix-server --test admin_reload admin_can_get_provider_and_model_by_id admin_can_list_providers_and_models_in_id_order admin_can_delete_provider_and_model -- --exact`

Expected: FAIL because the routes and handlers for provider/model `GET` and `DELETE` do not exist yet.

- [ ] **Step 3: Implement provider/model routes, shared state helpers, and handlers**

Extend `crates/aisix-server/src/admin/mod.rs` with shared read/delete logic:

```rust
use serde::{Serialize, de::DeserializeOwned};

pub async fn get_provider(&self, id: &str) -> Result<ProviderConfig, GatewayError> {
    self.get("providers", id).await
}

pub async fn list_providers(&self) -> Result<Vec<ProviderConfig>, GatewayError> {
    let mut providers: Vec<ProviderConfig> = self.list("providers").await?;
    providers.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(providers)
}

pub async fn delete_provider(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
    self.delete("providers", id).await
}

async fn get<T>(&self, collection: &str, id: &str) -> Result<T, GatewayError>
where
    T: DeserializeOwned,
{
    let mut etcd = self.etcd.lock().await;
    etcd
        .get_json(&self.prefix, collection, id)
        .await
        .map_err(internal_admin_error)?
        .ok_or_else(|| not_found_admin_error(collection, id))
}

async fn list<T>(&self, collection: &str) -> Result<Vec<T>, GatewayError>
where
    T: DeserializeOwned,
{
    let mut etcd = self.etcd.lock().await;
    etcd.list_json(&self.prefix, collection)
        .await
        .map_err(internal_admin_error)
}

async fn delete(&self, collection: &str, id: &str) -> Result<AdminWriteResult, GatewayError> {
    let mut etcd = self.etcd.lock().await;
    let delete = etcd
        .delete(&self.prefix, collection, id)
        .await
        .map_err(internal_admin_error)?;

    if !delete.deleted {
        return Err(not_found_admin_error(collection, id));
    }

    Ok(AdminWriteResult {
        id: id.to_string(),
        path: delete.key,
        revision: delete.revision,
    })
}

fn not_found_admin_error(collection: &str, id: &str) -> GatewayError {
    GatewayError {
        kind: ErrorKind::NotFound,
        message: format!("{collection} '{id}' not found"),
    }
}
```

Register the new routes in `crates/aisix-server/src/app.rs`:

```rust
use axum::{
    Router,
    routing::{delete, get, post, put},
};

let router = if state.admin.is_some() {
    router
        .route("/admin/providers", get(admin::providers::list_providers))
        .route(
            "/admin/providers/:id",
            get(admin::providers::get_provider)
                .put(admin::providers::put_provider)
                .delete(admin::providers::delete_provider),
        )
        .route("/admin/models", get(admin::models::list_models))
        .route(
            "/admin/models/:id",
            get(admin::models::get_model)
                .put(admin::models::put_model)
                .delete(admin::models::delete_model),
        )
} else {
    router
};
```

Add provider/model handlers in `crates/aisix-server/src/admin/providers.rs` and `crates/aisix-server/src/admin/models.rs`:

```rust
pub async fn list_providers(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.list_providers().await?))
}

pub async fn get_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProviderConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.get_provider(&id).await?))
}

pub async fn delete_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.delete_provider(&id).await?))
}
```

Mirror the same pattern for `ModelConfig` handlers in `models.rs`.

- [ ] **Step 4: Run the targeted provider/model tests and verify they pass**

Run: `cargo test -p aisix-server --test admin_reload admin_can_get_provider_and_model_by_id admin_can_list_providers_and_models_in_id_order admin_can_delete_provider_and_model -- --exact`

Expected: PASS for the three provider/model REST tests.

- [ ] **Step 5: Commit provider/model support**

```bash
git add crates/aisix-server/src/app.rs crates/aisix-server/src/admin/mod.rs crates/aisix-server/src/admin/providers.rs crates/aisix-server/src/admin/models.rs crates/aisix-server/tests/admin_reload.rs
git commit -m "feat: add provider and model admin read routes"
```

### Task 3: Add ApiKey and Policy REST Endpoints

**Files:**
- Modify: `crates/aisix-server/src/app.rs`
- Modify: `crates/aisix-server/src/admin/mod.rs`
- Modify: `crates/aisix-server/src/admin/apikeys.rs`
- Modify: `crates/aisix-server/src/admin/policies.rs`
- Modify: `crates/aisix-server/tests/admin_reload.rs`

- [ ] **Step 1: Write the failing server tests for apikey/policy reads and deletes**

Add these tests to `crates/aisix-server/tests/admin_reload.rs`:

```rust
#[tokio::test]
async fn admin_can_get_apikey_and_policy_by_id() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    app.clone()
        .oneshot(admin_put_request(
            "/admin/policies/default",
            json!({
                "id": "default",
                "rate_limit": {"rpm": 10}
            }),
        ))
        .await
        .unwrap();
    app.clone()
        .oneshot(admin_put_request(
            "/admin/apikeys/demo",
            json!({
                "id": "demo",
                "key": "live-token",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": "default"
            }),
        ))
        .await
        .unwrap();

    let apikey = app
        .clone()
        .oneshot(admin_get_request("/admin/apikeys/demo"))
        .await
        .unwrap();
    assert_eq!(apikey.status(), StatusCode::OK);

    let policy = app
        .oneshot(admin_get_request("/admin/policies/default"))
        .await
        .unwrap();
    assert_eq!(policy.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_can_list_apikeys_and_policies_in_id_order() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    app.clone()
        .oneshot(admin_put_request(
            "/admin/policies/z-policy",
            json!({
                "id": "z-policy",
                "rate_limit": {"rpm": 20}
            }),
        ))
        .await
        .unwrap();
    app.clone()
        .oneshot(admin_put_request(
            "/admin/policies/a-policy",
            json!({
                "id": "a-policy",
                "rate_limit": {"rpm": 10}
            }),
        ))
        .await
        .unwrap();

    let response = app
        .oneshot(admin_get_request("/admin/policies"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_can_delete_apikey_and_policy() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    app.clone()
        .oneshot(admin_put_request(
            "/admin/policies/default",
            json!({
                "id": "default",
                "rate_limit": {"rpm": 10}
            }),
        ))
        .await
        .unwrap();
    app.clone()
        .oneshot(admin_put_request(
            "/admin/apikeys/demo",
            json!({
                "id": "demo",
                "key": "live-token",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": "default"
            }),
        ))
        .await
        .unwrap();

    let delete_apikey = app
        .clone()
        .oneshot(admin_delete_request("/admin/apikeys/demo"))
        .await
        .unwrap();
    assert_eq!(delete_apikey.status(), StatusCode::OK);

    let delete_policy = app
        .oneshot(admin_delete_request("/admin/policies/default"))
        .await
        .unwrap();
    assert_eq!(delete_policy.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run the targeted apikey/policy tests and verify they fail**

Run: `cargo test -p aisix-server --test admin_reload admin_can_get_apikey_and_policy_by_id admin_can_list_apikeys_and_policies_in_id_order admin_can_delete_apikey_and_policy -- --exact`

Expected: FAIL because the apikey/policy routes and handlers do not exist yet.

- [ ] **Step 3: Implement the apikey/policy routes, state wrappers, and handlers**

Add typed wrappers in `crates/aisix-server/src/admin/mod.rs`:

```rust
pub async fn get_apikey(&self, id: &str) -> Result<ApiKeyConfig, GatewayError> {
    self.get("apikeys", id).await
}

pub async fn list_apikeys(&self) -> Result<Vec<ApiKeyConfig>, GatewayError> {
    let mut apikeys: Vec<ApiKeyConfig> = self.list("apikeys").await?;
    apikeys.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(apikeys)
}

pub async fn delete_apikey(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
    self.delete("apikeys", id).await
}

pub async fn get_policy(&self, id: &str) -> Result<PolicyConfig, GatewayError> {
    self.get("policies", id).await
}

pub async fn list_policies(&self) -> Result<Vec<PolicyConfig>, GatewayError> {
    let mut policies: Vec<PolicyConfig> = self.list("policies").await?;
    policies.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(policies)
}

pub async fn delete_policy(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
    self.delete("policies", id).await
}
```

Wire the routes in `crates/aisix-server/src/app.rs`:

```rust
.route("/admin/apikeys", get(admin::apikeys::list_apikeys))
.route(
    "/admin/apikeys/:id",
    get(admin::apikeys::get_apikey)
        .put(admin::apikeys::put_apikey)
        .delete(admin::apikeys::delete_apikey),
)
.route("/admin/policies", get(admin::policies::list_policies))
.route(
    "/admin/policies/:id",
    get(admin::policies::get_policy)
        .put(admin::policies::put_policy)
        .delete(admin::policies::delete_policy),
)
```

Add the matching handlers in `crates/aisix-server/src/admin/apikeys.rs` and `crates/aisix-server/src/admin/policies.rs`:

```rust
pub async fn list_apikeys(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeyConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.list_apikeys().await?))
}

pub async fn get_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ApiKeyConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.get_apikey(&id).await?))
}

pub async fn delete_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    Ok(Json(admin.delete_apikey(&id).await?))
}
```

Mirror the same structure for `PolicyConfig` handlers in `policies.rs`.

- [ ] **Step 4: Run the targeted apikey/policy tests and verify they pass**

Run: `cargo test -p aisix-server --test admin_reload admin_can_get_apikey_and_policy_by_id admin_can_list_apikeys_and_policies_in_id_order admin_can_delete_apikey_and_policy -- --exact`

Expected: PASS for the three apikey/policy REST tests.

- [ ] **Step 5: Commit apikey/policy support**

```bash
git add crates/aisix-server/src/app.rs crates/aisix-server/src/admin/mod.rs crates/aisix-server/src/admin/apikeys.rs crates/aisix-server/src/admin/policies.rs crates/aisix-server/tests/admin_reload.rs
git commit -m "feat: add apikey and policy admin read routes"
```

### Task 4: Harden Missing-Resource, Auth, and Ordering Semantics

**Files:**
- Modify: `crates/aisix-server/tests/admin_reload.rs`

- [ ] **Step 1: Write the failing negative-path integration tests**

Add these focused tests to `crates/aisix-server/tests/admin_reload.rs`:

```rust
#[tokio::test]
async fn admin_get_and_delete_return_not_found_for_missing_resources() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let get_missing = app
        .clone()
        .oneshot(admin_get_request("/admin/providers/missing"))
        .await
        .unwrap();
    assert_eq!(get_missing.status(), StatusCode::NOT_FOUND);

    let delete_missing = app
        .oneshot(admin_delete_request("/admin/providers/missing"))
        .await
        .unwrap();
    assert_eq!(delete_missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_get_and_delete_require_valid_x_admin_key() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let unauthorized_get = app
        .clone()
        .oneshot(Request::builder().method("GET").uri("/admin/providers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unauthorized_get.status(), StatusCode::UNAUTHORIZED);

    let unauthorized_delete = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/admin/providers/openai")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_delete.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_delete_removes_the_stored_key_from_etcd() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    app.clone()
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": {"secret_ref": "env:OPENAI_API_KEY"}
            }),
        ))
        .await
        .unwrap();

    let delete = app
        .oneshot(admin_delete_request("/admin/providers/openai"))
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);

    let stored = fixture.harness().get_json("/aisix/providers/openai").await.unwrap();
    assert!(stored.is_none());
}
```

- [ ] **Step 2: Run the targeted negative-path tests and verify they fail where behavior is still incomplete**

Run: `cargo test -p aisix-server --test admin_reload admin_get_and_delete_return_not_found_for_missing_resources admin_get_and_delete_require_valid_x_admin_key admin_delete_removes_the_stored_key_from_etcd -- --exact`

Expected: FAIL until the missing-resource mapping, auth path coverage, and delete assertions are fully implemented.

- [ ] **Step 3: Finish the helper cleanup inside the integration test module**

Refactor `crates/aisix-server/tests/admin_reload.rs` so all admin methods share one request builder:

```rust
fn admin_request(method: &str, path: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-admin-key", "test-admin-key");

    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

fn admin_put_request(path: &str, body: Value) -> Request<Body> {
    admin_request("PUT", path, Some(body))
}

fn admin_get_request(path: &str) -> Request<Body> {
    admin_request("GET", path, None)
}

fn admin_delete_request(path: &str) -> Request<Body> {
    admin_request("DELETE", path, None)
}
```

Also decode the collection responses and assert stable sorted ids explicitly:

```rust
let body = response.into_body().collect().await.unwrap().to_bytes();
let json: Value = serde_json::from_slice(&body).unwrap();
assert_eq!(json.as_array().unwrap()[0]["id"], "a-openai");
assert_eq!(json.as_array().unwrap()[1]["id"], "z-openai");
```

- [ ] **Step 4: Run the full relevant verification suite**

Run: `cargo test -p aisix-config --test admin_store -v && cargo test -p aisix-server --test admin_reload -v`

Expected: PASS for the new config-store tests and the full admin integration test file.

- [ ] **Step 5: Commit the hardening changes**

```bash
git add crates/aisix-server/tests/admin_reload.rs
git commit -m "test: cover admin restful semantics"
```

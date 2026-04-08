use std::sync::{Mutex, MutexGuard, OnceLock};

use aisix_config::{
    etcd_model::{ApiKeyConfig, ModelConfig, ProviderAuth, ProviderConfig, ProviderKind, RateLimitConfig},
    snapshot::CompiledSnapshot,
    watcher::initial_snapshot_handle,
};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn admin_can_create_provider_model_and_apikey_then_gateway_uses_reloaded_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(empty_snapshot(), true));

        let provider_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/providers/openai",
                json!(ProviderConfig {
                    id: "openai".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: upstream.base_url.clone(),
                    auth: ProviderAuth {
                        secret_ref: "env:OPENAI_API_KEY".to_string(),
                    },
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(provider_response.status(), StatusCode::OK);

        let model_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "openai".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(model_response.status(), StatusCode::OK);

        let key_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_admin",
                json!(ApiKeyConfig {
                    id: "vk_admin".to_string(),
                    key: "live-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(key_response.status(), StatusCode::OK);

        let response = app
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-aisix-provider")
                .and_then(|value| value.to_str().ok()),
            Some("openai")
        );
    })
    .await;
}

#[tokio::test]
async fn admin_put_seeds_from_live_snapshot_and_advances_revision() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(seed_snapshot(&upstream.base_url, 7), true));

        let response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_new",
                json!(ApiKeyConfig {
                    id: "vk_new".to_string(),
                    key: "new-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["revision"], 8);

        let old_key = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(old_key.status(), StatusCode::OK);

        let new_key = app
            .oneshot(chat_request("new-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(new_key.status(), StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn unrelated_admin_put_preserves_seeded_apikey_limit_config() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(
            seed_snapshot_with_limited_key(&upstream.base_url, 7),
            true,
        ));

        let first = app
            .clone()
            .oneshot(chat_request("limited-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let put = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/providers/openai",
                json!(ProviderConfig {
                    id: "openai".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: upstream.base_url.clone(),
                    auth: ProviderAuth {
                        secret_ref: "env:OPENAI_API_KEY".to_string(),
                    },
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::OK);

        let second = app
            .oneshot(chat_request("limited-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = second.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

#[tokio::test]
async fn admin_requires_valid_x_admin_key() {
    let app = aisix_server::app::build_router(test_state(empty_snapshot(), true));

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/providers/openai")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "id": "openai",
                        "kind": "openai",
                        "base_url": "http://127.0.0.1:1",
                        "auth": {"secret_ref": "env:OPENAI_API_KEY"},
                        "policy_id": null,
                        "rate_limit": null
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let invalid = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/providers/openai")
                .header("content-type", "application/json")
                .header("x-admin-key", "wrong-key")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "id": "openai",
                        "kind": "openai",
                        "base_url": "http://127.0.0.1:1",
                        "auth": {"secret_ref": "env:OPENAI_API_KEY"},
                        "policy_id": null,
                        "rate_limit": null
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_rejects_path_and_body_id_mismatch() {
    let app = aisix_server::app::build_router(test_state(empty_snapshot(), true));

    let response = app
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "different-provider".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "http://127.0.0.1:1".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"]["message"],
        "path id 'openai' does not match body id 'different-provider'"
    );
}

#[tokio::test]
async fn admin_update_reloads_key_rate_limit_without_poisoning_current_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(empty_snapshot(), true));

        seed_valid_runtime_config(&app, &upstream.base_url, Some(1)).await;

        let first = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_admin",
                json!(ApiKeyConfig {
                    id: "vk_admin".to_string(),
                    key: "live-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: Some(RateLimitConfig {
                        rpm: Some(2),
                        tpm: None,
                        concurrency: None,
                    }),
                }),
            ))
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);

        let second = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);

        let invalid_update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "missing-provider".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_update.status(), StatusCode::BAD_REQUEST);

        let third = app
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = third.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

async fn seed_valid_runtime_config(app: &axum::Router, base_url: &str, rpm: Option<u64>) {
    let provider = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: base_url.to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(provider.status(), StatusCode::OK);

    let model = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/models/gpt-4o-mini",
            json!(ModelConfig {
                id: "gpt-4o-mini".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(model.status(), StatusCode::OK);

    let key = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/apikeys/vk_admin",
            json!(ApiKeyConfig {
                id: "vk_admin".to_string(),
                key: "live-token".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: rpm.map(|rpm| RateLimitConfig {
                    rpm: Some(rpm),
                    tpm: None,
                    concurrency: None,
                }),
            }),
        ))
        .await
        .unwrap();
    assert_eq!(key.status(), StatusCode::OK);
}

fn test_state(snapshot: CompiledSnapshot, ready: bool) -> aisix_server::app::ServerState {
    let snapshot = initial_snapshot_handle(snapshot);
    aisix_server::app::ServerState {
        app: AppState::new(snapshot.clone(), ready),
        providers: ProviderRegistry::default(),
        admin: Some(aisix_server::admin::AdminState::new(
            "/aisix".to_string(),
            vec!["test-admin-key".to_string()],
            snapshot,
        )),
    }
}

fn empty_snapshot() -> CompiledSnapshot {
    CompiledSnapshot {
        revision: 0,
        keys_by_token: Default::default(),
        apikeys_by_id: Default::default(),
        providers_by_id: Default::default(),
        models_by_name: Default::default(),
        policies_by_id: Default::default(),
        provider_limits: Default::default(),
        model_limits: Default::default(),
        key_limits: Default::default(),
    }
}

fn seed_snapshot(base_url: &str, revision: i64) -> CompiledSnapshot {
    let mut snapshot = empty_snapshot();
    snapshot.revision = revision;
    snapshot.keys_by_token.insert(
        "live-token".to_string(),
        aisix_types::entities::KeyMeta {
            key_id: "vk_admin".to_string(),
            user_id: None,
            customer_id: None,
            alias: None,
            expires_at: None,
            allowed_models: vec!["gpt-4o-mini".to_string()],
        },
    );
    snapshot.apikeys_by_id.insert(
        "vk_admin".to_string(),
        ApiKeyConfig {
            id: "vk_admin".to_string(),
            key: "live-token".to_string(),
            allowed_models: vec!["gpt-4o-mini".to_string()],
            policy_id: None,
            rate_limit: None,
        },
    );
    snapshot.providers_by_id.insert(
        "openai".to_string(),
        ProviderConfig {
            id: "openai".to_string(),
            kind: ProviderKind::OpenAi,
            base_url: base_url.to_string(),
            auth: ProviderAuth {
                secret_ref: "env:OPENAI_API_KEY".to_string(),
            },
            policy_id: None,
            rate_limit: None,
        },
    );
    snapshot.models_by_name.insert(
        "gpt-4o-mini".to_string(),
        ModelConfig {
            id: "gpt-4o-mini".to_string(),
            provider_id: "openai".to_string(),
            upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
            policy_id: None,
            rate_limit: None,
        },
    );
    snapshot
}

fn seed_snapshot_with_limited_key(base_url: &str, revision: i64) -> CompiledSnapshot {
    let mut snapshot = seed_snapshot(base_url, revision);
    snapshot.keys_by_token.insert(
        "limited-token".to_string(),
        aisix_types::entities::KeyMeta {
            key_id: "vk_limited".to_string(),
            user_id: None,
            customer_id: None,
            alias: None,
            expires_at: None,
            allowed_models: vec!["gpt-4o-mini".to_string()],
        },
    );
    snapshot.apikeys_by_id.insert(
        "vk_limited".to_string(),
        ApiKeyConfig {
            id: "vk_limited".to_string(),
            key: "limited-token".to_string(),
            allowed_models: vec!["gpt-4o-mini".to_string()],
            policy_id: None,
            rate_limit: Some(RateLimitConfig {
                rpm: Some(1),
                tpm: None,
                concurrency: None,
            }),
        },
    );
    snapshot.key_limits.insert(
        "vk_limited".to_string(),
        aisix_config::snapshot::ResolvedLimits {
            rpm: Some(1),
            tpm: None,
            concurrency: None,
        },
    );
    snapshot
}

fn admin_put_request(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(path)
        .header("content-type", "application/json")
        .header("x-admin-key", "test-admin-key")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn chat_request(token: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            }))
            .unwrap(),
        ))
        .unwrap()
}

async fn with_env_var<F, Fut, T>(name: &str, value: Option<&str>, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _restore = EnvVarGuard::set(name, value);

    f().await
}

struct EnvVarGuard {
    _lock: MutexGuard<'static, ()>,
    name: String,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(name: &str, value: Option<&str>) -> Self {
        let lock = env_var_lock().lock().unwrap();
        let previous = std::env::var(name).ok();
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }

        Self {
            _lock: lock,
            name: name.to_string(),
            previous,
        }
    }
}

fn env_var_lock() -> &'static Mutex<()> {
    static ENV_VAR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_VAR_LOCK.get_or_init(|| Mutex::new(()))
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(&self.name, previous),
            None => std::env::remove_var(&self.name),
        }
    }
}

struct MockUpstream {
    base_url: String,
}

async fn spawn_openai_mock() -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<Incoming>| async move {
                    let _body = request.into_body().collect().await.unwrap().to_bytes();

                    Ok::<_, std::convert::Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(http_body_util::Full::new(Bytes::from(
                                serde_json::to_vec(&json!({
                                    "id": "chatcmpl-admin-test",
                                    "object": "chat.completion",
                                    "created": 1,
                                    "model": "gpt-4o-mini-2024-07-18",
                                    "choices": [{
                                        "index": 0,
                                        "message": {"role": "assistant", "content": "hi"},
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 1,
                                        "completion_tokens": 1,
                                        "total_tokens": 2
                                    }
                                }))
                                .unwrap(),
                            )))
                            .unwrap(),
                    )
                });

                hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                    .unwrap();
            });
        }
    });

    MockUpstream {
        base_url: format!("http://{}", address),
    }
}

use std::{
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

use aisix_config::etcd_model::{
    ApiKeyConfig, ModelConfig, ProviderAuth, ProviderConfig, ProviderKind, RateLimitConfig,
};
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

mod support {
    pub mod etcd {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../aisix-config/tests/support/etcd.rs"));
    }
}

#[tokio::test]
async fn admin_can_create_provider_model_and_apikey_then_gateway_uses_reloaded_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start().await;
        let app = fixture.router();

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

        let response = poll_until_response(|| {
            let app = app.clone();
            async move { app.oneshot(chat_request("live-token", "gpt-4o-mini")).await.unwrap() }
        })
        .await;
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
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, None)
            .await;
        let app = fixture.router();

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
        assert_eq!(json["revision"].as_i64().unwrap(), fixture.seeded_revision() + 1);

        let old_key = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(old_key.status(), StatusCode::OK);

        let new_key = poll_until_response(|| {
            let app = app.clone();
            async move { app.oneshot(chat_request("new-token", "gpt-4o-mini")).await.unwrap() }
        })
        .await;
        assert_eq!(new_key.status(), StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn unrelated_admin_put_preserves_seeded_apikey_limit_config() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded_with_limited_key(&upstream.base_url).await;
        let app = fixture.router();

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

        let second = poll_until_status(|| {
            let app = app.clone();
            async move { app.oneshot(chat_request("limited-token", "gpt-4o-mini")).await.unwrap() }
        }, StatusCode::TOO_MANY_REQUESTS)
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = second.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

#[tokio::test]
async fn admin_requires_valid_x_admin_key() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

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
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

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
async fn admin_put_persists_provider_in_etcd_and_returns_write_revision() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://api.openai.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["id"], "openai");
    assert_eq!(json["path"], "/aisix/providers/openai");
    assert!(json["revision"].as_i64().unwrap() > 0);

    let stored = fixture
        .harness()
        .get_json("/aisix/providers/openai")
        .await
        .expect("etcd read should succeed")
        .expect("provider should be persisted");
    assert_eq!(stored["id"], "openai");
    assert_eq!(stored["base_url"], "https://api.openai.com");
}

#[tokio::test]
async fn admin_update_reloads_key_rate_limit_without_poisoning_current_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, Some(1)).await;
        let app = fixture.router();

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

        let second = poll_until_response(|| {
            let app = app.clone();
            async move { app.oneshot(chat_request("live-token", "gpt-4o-mini")).await.unwrap() }
        })
        .await;
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
        assert_eq!(invalid_update.status(), StatusCode::OK);

        let third = poll_until_status(|| {
            let app = app.clone();
            async move { app.oneshot(chat_request("live-token", "gpt-4o-mini")).await.unwrap() }
        }, StatusCode::TOO_MANY_REQUESTS)
        .await;
        assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = third.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

struct LiveEtcdTestApp {
    router: axum::Router,
    _state: aisix_server::app::ServerState,
    harness: support::etcd::EtcdHarness,
    seeded_revision: i64,
}

impl LiveEtcdTestApp {
    async fn start() -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        Self::start_with_harness(harness, 0).await
    }

    async fn start_seeded(base_url: &str, rpm: Option<u64>) -> Self {
        Self::start_with_seed(Some((base_url, rpm))).await
    }

    async fn start_seeded_with_limited_key(base_url: &str) -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        let _seeded_revision = seed_valid_runtime_config_in_etcd(&harness, base_url, None).await;
        let seeded_revision = seed_limited_runtime_key_in_etcd(&harness).await;

        Self::start_with_harness(harness, seeded_revision).await
    }

    async fn start_with_seed(seed: Option<(&str, Option<u64>)>) -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        let mut seeded_revision = 0;
        if let Some((base_url, rpm)) = seed {
            seeded_revision = seed_valid_runtime_config_in_etcd(&harness, base_url, rpm).await;
        }
        Self::start_with_harness(harness, seeded_revision).await
    }

    async fn start_with_harness(harness: support::etcd::EtcdHarness, seeded_revision: i64) -> Self {
        let startup = test_startup_config(harness.config());
        let app = aisix_runtime::bootstrap::bootstrap(&startup)
            .await
            .expect("runtime bootstrap should succeed");
        let admin = aisix_server::admin::AdminState::from_startup_config(&startup)
            .await
            .expect("admin state should initialize")
            .expect("admin state should be enabled");
        let state = aisix_server::app::ServerState {
            app,
            providers: ProviderRegistry::default(),
            admin: Some(admin),
        };

        Self {
            router: aisix_server::app::build_router(state.clone()),
            _state: state,
            harness,
            seeded_revision,
        }
    }

    fn router(&self) -> axum::Router {
        self.router.clone()
    }

    fn harness(&self) -> &support::etcd::EtcdHarness {
        &self.harness
    }

    fn seeded_revision(&self) -> i64 {
        self.seeded_revision
    }
}

async fn seed_valid_runtime_config_in_etcd(
    harness: &support::etcd::EtcdHarness,
    base_url: &str,
    rpm: Option<u64>,
) -> i64 {
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: base_url.to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
            }),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!(ModelConfig {
                id: "gpt-4o-mini".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                policy_id: None,
                rate_limit: None,
            }),
        )
        .await
        .expect("model fixture should be written");
    harness
        .put_json(
            "/aisix/apikeys/vk_admin",
            &json!(ApiKeyConfig {
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
        )
        .await
        .expect("apikey fixture should be written")
}

async fn seed_limited_runtime_key_in_etcd(harness: &support::etcd::EtcdHarness) -> i64 {
    harness
        .put_json(
            "/aisix/apikeys/vk_limited",
            &json!(ApiKeyConfig {
                id: "vk_limited".to_string(),
                key: "limited-token".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: Some(RateLimitConfig {
                    rpm: Some(1),
                    tpm: None,
                    concurrency: None,
                }),
            }),
        )
        .await
        .expect("limited apikey fixture should be written")
}

fn test_startup_config(etcd: aisix_config::startup::EtcdConfig) -> aisix_config::startup::StartupConfig {
    aisix_config::startup::StartupConfig {
        server: aisix_config::startup::ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd,
        redis: aisix_config::startup::RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: aisix_config::startup::LogConfig {
            level: "info".to_string(),
        },
        runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
        deployment: aisix_config::startup::DeploymentConfig {
            admin: aisix_config::startup::AdminConfig {
                enabled: true,
                admin_keys: vec![aisix_config::startup::AdminKey {
                    key: "test-admin-key".to_string(),
                }],
            },
        },
    }
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

async fn poll_until_response<F, Fut>(request: F) -> axum::response::Response
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = axum::response::Response>,
{
    poll_until_status(request, StatusCode::OK).await
}

async fn poll_until_status<F, Fut>(mut request: F, expected: StatusCode) -> axum::response::Response
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = axum::response::Response>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = request().await;
        if response.status() == expected {
            return response;
        }

        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body).into_owned();

        if Instant::now() >= deadline {
            panic!(
                "response did not reach expected status {expected} before timeout; last status: {status}; last body: {body}"
            );
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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

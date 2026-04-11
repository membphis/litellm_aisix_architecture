use std::sync::{Mutex, MutexGuard, OnceLock};

use aisix_config::{
    etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind},
    snapshot::CompiledSnapshot,
    watcher::initial_snapshot_handle,
};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use aisix_types::entities::KeyMeta;
use aisix_types::request::ChatRequest;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn health_and_ready_are_exposed() {
    let app = app_with_snapshot(empty_snapshot());

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let ready = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
}

#[tokio::test]
async fn ready_returns_503_when_state_is_not_ready() {
    let app = aisix_server::app::build_router(test_state(empty_snapshot(), false));

    let ready = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn invalid_virtual_key_returns_401() {
    let app = app_with_snapshot(auth_snapshot(None, "https://example.invalid"));

    let response = app
        .oneshot(chat_request_with_auth(
            Some("Bearer invalid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_authorization_returns_401() {
    let app = app_with_snapshot(auth_snapshot(None, "https://example.invalid"));

    let response = app
        .oneshot(chat_request_with_auth(None, "gpt-4o-mini"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn malformed_authorization_returns_401() {
    let app = app_with_snapshot(auth_snapshot(None, "https://example.invalid"));

    let response = app
        .oneshot(chat_request_with_auth(
            Some("Token valid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_key_returns_401() {
    let app = app_with_snapshot(auth_snapshot(
        Some(Utc::now() - Duration::minutes(5)),
        "https://example.invalid",
    ));

    let response = app
        .oneshot(chat_request_with_auth(
            Some("Bearer valid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn exact_expiry_boundary_returns_401() {
    let app = app_with_snapshot(auth_snapshot(Some(Utc::now()), "https://example.invalid"));

    let response = app
        .oneshot(chat_request_with_auth(
            Some("Bearer valid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn happy_path_auth_and_route_returns_200() {
    let upstream = spawn_openai_mock().await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = app_with_snapshot(auth_snapshot(None, &upstream.base_url));

        app.oneshot(chat_request_with_auth(
            Some("Bearer valid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn lowercase_bearer_scheme_returns_200() {
    let upstream = spawn_openai_mock().await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = app_with_snapshot(auth_snapshot(None, &upstream.base_url));

        app.oneshot(chat_request_with_auth(
            Some("bearer valid-token"),
            "gpt-4o-mini",
        ))
        .await
        .unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn disallowed_model_returns_403() {
    let app = app_with_snapshot(auth_snapshot(None, "https://example.invalid"));

    let response = app
        .oneshot(chat_request_with_auth(
            Some("Bearer valid-token"),
            "gpt-4.1",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

fn app_with_snapshot(snapshot: CompiledSnapshot) -> axum::Router {
    aisix_server::app::build_router(test_state(snapshot, true))
}

fn test_state(snapshot: CompiledSnapshot, ready: bool) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), ready, false),
        providers: ProviderRegistry::default(),
        admin: None,
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
        provider_cache_modes: Default::default(),
        model_cache_modes: Default::default(),
    }
}

fn auth_snapshot(expires_at: Option<chrono::DateTime<Utc>>, base_url: &str) -> CompiledSnapshot {
    let mut snapshot = empty_snapshot();
    snapshot.keys_by_token.insert(
        "valid-token".to_string(),
        KeyMeta {
            key_id: "vk_123".to_string(),
            user_id: None,
            customer_id: None,
            alias: Some("test-key".to_string()),
            expires_at,
            allowed_models: vec!["gpt-4o-mini".to_string()],
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
            cache: None,
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
            cache: None,
        },
    );
    snapshot
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
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);

        let service = service_fn(move |request: hyper::Request<Incoming>| async move {
            let _body = request.into_body().collect().await.unwrap().to_bytes();

            Ok::<_, std::convert::Infallible>(
                hyper::Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(http_body_util::Full::new(Bytes::from(
                        serde_json::to_vec(&json!({
                            "id": "chatcmpl-auth-test",
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

    MockUpstream {
        base_url: format!("http://{}", address),
    }
}

fn chat_request_with_auth(authorization: Option<&str>, model: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");

    if let Some(authorization) = authorization {
        builder = builder.header("authorization", authorization);
    }

    builder
        .body(Body::from(
            serde_json::to_vec(&chat_request(model)).unwrap(),
        ))
        .unwrap()
}

fn chat_request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: vec![json!({"role": "user", "content": "hello"})],
        stream: false,
    }
}

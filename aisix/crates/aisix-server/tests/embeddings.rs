use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard, OnceLock,
};

use aisix_config::{
    etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind},
    snapshot::CompiledSnapshot,
    watcher::initial_snapshot_handle,
};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use aisix_types::entities::KeyMeta;
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
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn embeddings_reuse_the_json_pipeline() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(
            &upstream.base_url,
            ProviderKind::OpenAi,
        )));

        app.oneshot(embeddings_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "list");
    assert_eq!(capture.path(), Some("/v1/embeddings".to_string()));
    assert_eq!(
        capture.model(),
        Some("text-embedding-3-small-upstream".to_string())
    );
}

#[tokio::test]
async fn azure_embeddings_use_api_key_header() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let response = with_env_var("OPENAI_API_KEY", Some("test-azure-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(
            &upstream.base_url,
            ProviderKind::AzureOpenAi,
        )));

        app.oneshot(embeddings_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(capture.path(), Some("/v1/embeddings".to_string()));
    assert_eq!(capture.api_key(), Some("test-azure-key".to_string()));
    assert_eq!(capture.authorization(), None);
}

fn test_state(snapshot: CompiledSnapshot) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), true, false),
        providers: ProviderRegistry::default(),
        admin: None,
    }
}

fn snapshot_for_upstream(base_url: &str, kind: ProviderKind) -> CompiledSnapshot {
    let mut snapshot = CompiledSnapshot {
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
    };

    snapshot.keys_by_token.insert(
        "valid-token".to_string(),
        KeyMeta {
            key_id: "vk_123".to_string(),
            user_id: None,
            customer_id: None,
            alias: Some("test-key".to_string()),
            expires_at: None,
            allowed_models: vec!["text-embedding-3-small".to_string()],
        },
    );
    snapshot.providers_by_id.insert(
        "openai".to_string(),
        ProviderConfig {
            id: "openai".to_string(),
            kind,
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
        "text-embedding-3-small".to_string(),
        ModelConfig {
            id: "text-embedding-3-small".to_string(),
            provider_id: "openai".to_string(),
            upstream_model: "text-embedding-3-small-upstream".to_string(),
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

fn embeddings_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/embeddings")
        .header("content-type", "application/json")
        .header("authorization", "Bearer valid-token")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "text-embedding-3-small",
                "input": "hello"
            }))
            .unwrap(),
        ))
        .unwrap()
}

#[derive(Default)]
struct CapturedRequest {
    path: std::sync::Mutex<Option<String>>,
    authorization: std::sync::Mutex<Option<String>>,
    api_key: std::sync::Mutex<Option<String>>,
    model: std::sync::Mutex<Option<String>>,
    hits: AtomicUsize,
}

impl CapturedRequest {
    fn path(&self) -> Option<String> {
        self.path.lock().unwrap().clone()
    }

    fn authorization(&self) -> Option<String> {
        self.authorization.lock().unwrap().clone()
    }

    fn api_key(&self) -> Option<String> {
        self.api_key.lock().unwrap().clone()
    }

    fn model(&self) -> Option<String> {
        self.model.lock().unwrap().clone()
    }
}

struct MockUpstream {
    base_url: String,
}

async fn spawn_openai_mock(capture: Arc<CapturedRequest>) -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let capture = capture.clone();

            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<Incoming>| {
                    let capture = capture.clone();
                    async move {
                        capture.hits.fetch_add(1, Ordering::SeqCst);
                        *capture.path.lock().unwrap() = Some(request.uri().path().to_string());
                        *capture.authorization.lock().unwrap() = request
                            .headers()
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        *capture.api_key.lock().unwrap() = request
                            .headers()
                            .get("api-key")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);

                        let body = request.into_body().collect().await.unwrap().to_bytes();
                        let json: Value = serde_json::from_slice(&body).unwrap();
                        *capture.model.lock().unwrap() = json["model"].as_str().map(str::to_string);

                        let response_body = if capture.path() == Some("/v1/embeddings".to_string())
                        {
                            serde_json::to_vec(&json!({
                                "object": "list",
                                "data": [{
                                    "object": "embedding",
                                    "embedding": [0.1, 0.2],
                                    "index": 0
                                }],
                                "model": json["model"],
                                "usage": {
                                    "prompt_tokens": 3,
                                    "total_tokens": 3
                                }
                            }))
                            .unwrap()
                        } else {
                            serde_json::to_vec(&json!({
                                "id": "chatcmpl-test",
                                "object": "chat.completion",
                                "created": 1,
                                "model": "gpt-4o-mini-2024-07-18",
                                "choices": [{
                                    "index": 0,
                                    "message": {"role": "assistant", "content": "hi"},
                                    "finish_reason": "stop"
                                }],
                                "usage": {
                                    "prompt_tokens": 12,
                                    "completion_tokens": 7,
                                    "total_tokens": 19
                                }
                            }))
                            .unwrap()
                        };

                        Ok::<_, std::convert::Infallible>(
                            hyper::Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(http_body_util::Full::new(Bytes::from(response_body)))
                                .unwrap(),
                        )
                    }
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

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard, OnceLock,
};

use aisix_config::{
    etcd_model::{
        CacheMode, CachePolicyConfig, ModelConfig, ProviderAuth, ProviderConfig, ProviderKind,
    },
    snapshot::CompiledSnapshot,
    watcher::initial_snapshot_handle,
};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use aisix_types::{entities::KeyMeta, usage::Usage};
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
async fn chat_non_stream_proxies_openai_compatible_json() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(
            snapshot_for_upstream(&upstream.base_url),
            true,
            false,
        ));

        app.oneshot(chat_request()).await.unwrap()
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

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["usage"]["prompt_tokens"], 12);
    assert_eq!(json["usage"]["completion_tokens"], 7);
    assert_eq!(capture.path(), Some("/v1/chat/completions".to_string()));
    assert_eq!(
        capture.authorization(),
        Some("Bearer test-openai-key".to_string())
    );
    assert_eq!(capture.model(), Some("gpt-4o-mini-2024-07-18".to_string()));
}

#[tokio::test]
async fn azure_chat_json_path_uses_api_key_header() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(
            snapshot_for_upstream_kind(&upstream.base_url, ProviderKind::AzureOpenAi),
            true,
            false,
        ));

        app.oneshot(chat_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(capture.path(), Some("/v1/chat/completions".to_string()));
    assert_eq!(capture.authorization(), None);
    assert_eq!(capture.api_key(), Some("test-openai-key".to_string()));
}

#[tokio::test]
async fn chat_response_uses_provider_config_id_and_preserves_usage() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(
            snapshot_for_provider_id(&upstream.base_url, ProviderKind::OpenAi, "openai-primary"),
            true,
            false,
        ));

        app.oneshot(chat_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-aisix-provider")
            .and_then(|value| value.to_str().ok()),
        Some("openai-primary")
    );

    let usage = response
        .extensions()
        .get::<Usage>()
        .cloned()
        .expect("usage should be attached to response extensions");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 7);
}

#[tokio::test]
async fn repeated_non_stream_chat_hits_memory_cache() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (state, first, second) =
        with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
            let state = test_state(snapshot_for_upstream(&upstream.base_url), true, true);
            let app = aisix_server::app::build_router(state.clone());

            let first = app.clone().oneshot(chat_request()).await.unwrap();
            let second = app.oneshot(chat_request()).await.unwrap();

            (state, first, second)
        })
        .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("false")
    );

    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        second
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    assert_eq!(
        second
            .headers()
            .get("x-aisix-provider")
            .and_then(|value| value.to_str().ok()),
        Some("openai")
    );

    let cached_usage = second
        .extensions()
        .get::<Usage>()
        .cloned()
        .expect("cached usage should be attached to response extensions");
    assert_eq!(cached_usage.input_tokens, 12);
    assert_eq!(cached_usage.output_tokens, 7);
    assert_eq!(
        state
            .app
            .usage_recorder
            .total_for("usage:key:vk_123:input_tokens"),
        24
    );
    assert_eq!(
        state
            .app
            .usage_recorder
            .total_for("usage:key:vk_123:output_tokens"),
        14
    );
    assert_eq!(capture.hits(), 1);
}

#[tokio::test]
async fn cache_entry_is_invalidated_when_snapshot_config_changes() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(
            snapshot_for_model_target(&upstream.base_url, 1, "openai", "gpt-4o-mini-2024-07-18"),
            true,
            true,
        );
        let app = aisix_server::app::build_router(state.clone());

        let first = app.clone().oneshot(chat_request()).await.unwrap();

        state
            .app
            .snapshot
            .store(std::sync::Arc::new(snapshot_for_model_target(
                &upstream.base_url,
                2,
                "openai-v2",
                "gpt-4o-mini-2024-08-01",
            )));

        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("false")
    );

    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        second
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("false")
    );
    assert_eq!(
        second
            .headers()
            .get("x-aisix-provider")
            .and_then(|value| value.to_str().ok()),
        Some("openai-v2")
    );
    assert_eq!(capture.hits(), 2);
    assert_eq!(capture.model(), Some("gpt-4o-mini-2024-08-01".to_string()));
}

#[tokio::test]
async fn non_stream_chat_skips_cache_when_globally_disabled_and_resources_inherit() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(snapshot_for_upstream(&upstream.base_url), true, false);
        let app = aisix_server::app::build_router(state);

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    assert!(first.headers().get("x-aisix-cache-hit").is_none());
    assert!(second.headers().get("x-aisix-cache-hit").is_none());
    assert_eq!(capture.hits(), 2);
}

#[tokio::test]
async fn non_stream_chat_hits_cache_when_globally_enabled_and_resources_inherit() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(snapshot_for_upstream(&upstream.base_url), true, true);
        let app = aisix_server::app::build_router(state);

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert_eq!(
        first
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("false")
    );
    assert_eq!(
        second
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    assert_eq!(capture.hits(), 1);
}

#[tokio::test]
async fn provider_cache_enabled_overrides_global_default_disabled() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(
            snapshot_for_cache_modes(&upstream.base_url, Some(CacheMode::Enabled), None),
            true,
            false,
        );
        let app = aisix_server::app::build_router(state);

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("false")
    );

    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        second
            .headers()
            .get("x-aisix-cache-hit")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    assert_eq!(capture.hits(), 1);
}

#[tokio::test]
async fn provider_cache_disabled_overrides_global_default_enabled() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(
            snapshot_for_cache_modes(&upstream.base_url, Some(CacheMode::Disabled), None),
            true,
            true,
        );
        let app = aisix_server::app::build_router(state);

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert_eq!(first.status(), StatusCode::OK);
    assert!(first.headers().get("x-aisix-cache-hit").is_none());

    assert_eq!(second.status(), StatusCode::OK);
    assert!(second.headers().get("x-aisix-cache-hit").is_none());
    assert_eq!(capture.hits(), 2);
}

#[tokio::test]
async fn model_cache_mode_overrides_provider_cache_mode() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let (first, second) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(
            snapshot_for_cache_modes(
                &upstream.base_url,
                Some(CacheMode::Enabled),
                Some(CacheMode::Disabled),
            ),
            true,
            false,
        );
        let app = aisix_server::app::build_router(state);

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        let second = app.oneshot(chat_request()).await.unwrap();

        (first, second)
    })
    .await;

    assert!(first.headers().get("x-aisix-cache-hit").is_none());
    assert!(second.headers().get("x-aisix-cache-hit").is_none());
    assert_eq!(capture.hits(), 2);
}

fn test_state(
    snapshot: CompiledSnapshot,
    ready: bool,
    default_cache_enabled: bool,
) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(
            initial_snapshot_handle(snapshot),
            ready,
            default_cache_enabled,
        ),
        providers: ProviderRegistry::default(),
        admin: None,
    }
}

fn snapshot_for_upstream(base_url: &str) -> CompiledSnapshot {
    snapshot_for_upstream_kind(base_url, ProviderKind::OpenAi)
}

fn snapshot_for_upstream_kind(base_url: &str, kind: ProviderKind) -> CompiledSnapshot {
    snapshot_for_provider(base_url, 0, kind, "openai", "gpt-4o-mini-2024-07-18")
}

fn snapshot_for_provider_id(
    base_url: &str,
    kind: ProviderKind,
    provider_id: &str,
) -> CompiledSnapshot {
    snapshot_for_provider(base_url, 0, kind, provider_id, "gpt-4o-mini-2024-07-18")
}

fn snapshot_for_model_target(
    base_url: &str,
    revision: i64,
    provider_id: &str,
    upstream_model: &str,
) -> CompiledSnapshot {
    snapshot_for_provider(
        base_url,
        revision,
        ProviderKind::OpenAi,
        provider_id,
        upstream_model,
    )
}

fn snapshot_for_cache_modes(
    base_url: &str,
    provider_mode: Option<CacheMode>,
    model_mode: Option<CacheMode>,
) -> CompiledSnapshot {
    let mut snapshot = snapshot_for_upstream(base_url);

    if let Some(provider) = snapshot.providers_by_id.get_mut("openai") {
        provider.cache = provider_mode.map(|mode| CachePolicyConfig { mode });
    }

    if let Some(model) = snapshot.models_by_name.get_mut("gpt-4o-mini") {
        model.cache = model_mode.map(|mode| CachePolicyConfig { mode });
    }

    snapshot.provider_cache_modes.insert(
        "openai".to_string(),
        provider_mode.unwrap_or(CacheMode::Inherit),
    );
    snapshot.model_cache_modes.insert(
        "gpt-4o-mini".to_string(),
        model_mode.unwrap_or(CacheMode::Inherit),
    );

    snapshot
}

fn snapshot_for_provider(
    base_url: &str,
    revision: i64,
    kind: ProviderKind,
    provider_id: &str,
    upstream_model: &str,
) -> CompiledSnapshot {
    let mut snapshot = CompiledSnapshot {
        revision,
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
            allowed_models: vec!["gpt-4o-mini".to_string()],
        },
    );
    snapshot.providers_by_id.insert(
        provider_id.to_string(),
        ProviderConfig {
            id: provider_id.to_string(),
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
        "gpt-4o-mini".to_string(),
        ModelConfig {
            id: "gpt-4o-mini".to_string(),
            provider_id: provider_id.to_string(),
            upstream_model: upstream_model.to_string(),
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

fn chat_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer valid-token")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
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

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
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

                    let response_body = serde_json::to_vec(&json!({
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
                    .unwrap();

                    Ok::<_, std::convert::Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(http_body_util::Full::new(Bytes::from(response_body)))
                            .unwrap(),
                    )
                }
            });

            tokio::spawn(async move {
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

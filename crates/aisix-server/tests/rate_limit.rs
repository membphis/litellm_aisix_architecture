use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tokio::sync::{oneshot, Notify};

use aisix_config::{
    etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind, RateLimitConfig},
    snapshot::CompiledSnapshot,
    startup::{
        AdminConfig, CacheConfig, CacheDefaultMode, DeploymentConfig, EtcdConfig, LogConfig,
        RedisConfig, RuntimeConfig, ServerConfig, StartupConfig,
    },
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
use tokio::time::{timeout, Duration};
use tower::ServiceExt;

mod support {
    pub mod etcd {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../aisix-config/tests/support/etcd.rs"
        ));
    }
}

#[tokio::test]
async fn inline_rpm_limit_triggers_429() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;
    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_with_inline_rpm_limit(
            &upstream.base_url,
            1,
        )));

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = app.oneshot(chat_request()).await.unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = second.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
        assert_eq!(capture.hits(), 1);
    })
    .await;
}

#[tokio::test]
async fn redis_failure_degrades_to_shadow_limiter() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = aisix_runtime::bootstrap::bootstrap(&broken_redis_config(harness.config()))
            .await
            .unwrap();
        let snapshot = snapshot_with_inline_rpm_limit(&upstream.base_url, 1);
        state.snapshot.store(std::sync::Arc::new(snapshot));
        let app = aisix_server::app::build_router(aisix_server::app::ServerState {
            app: state,
            providers: ProviderRegistry::default(),
            admin: None,
        });

        let first = app.clone().oneshot(chat_request()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = app.oneshot(chat_request()).await.unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(capture.hits(), 1);
    })
    .await;
}

#[tokio::test]
async fn key_rpm_still_inherits_provider_concurrency_limit() {
    let gate = Arc::new(BlockedUpstream::default());
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_blocked_openai_mock(capture.clone(), gate.clone()).await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(snapshot_with_mixed_scope_limits(&upstream.base_url));
        let app = aisix_server::app::build_router(state.clone());
        let (status_tx, status_rx) = oneshot::channel();
        let (second_status_tx, second_status_rx) = oneshot::channel();
        let first_app = app.clone();
        let second_app = app.clone();

        let first_request = tokio::spawn(async move {
            let response = first_app.oneshot(chat_request()).await.unwrap();
            let _ = status_tx.send(response.status());
        });

        gate.wait_until_started().await;

        let second_request = tokio::spawn(async move {
            let response = second_app.oneshot(chat_request()).await.unwrap();
            let _ = second_status_tx.send(response.status());
        });

        let second_status = timeout(Duration::from_millis(500), second_status_rx).await;

        gate.release();

        let second_status = match second_status {
            Ok(Ok(status)) => status,
            Ok(Err(_)) => panic!("second request status channel dropped unexpectedly"),
            Err(_) => {
                panic!("second request did not fail fast; concurrency fallback was not inherited")
            }
        };

        assert_eq!(second_status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(status_rx.await.unwrap(), StatusCode::OK);
        first_request.await.unwrap();
        second_request.await.unwrap();
        assert_eq!(capture.hits(), 1);
    })
    .await;
}

#[tokio::test]
async fn failed_upstream_does_not_record_usage() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_error_openai_mock(capture.clone()).await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(snapshot_with_inline_rpm_limit(&upstream.base_url, 10));
        let app = aisix_server::app::build_router(state.clone());

        let response = app.oneshot(chat_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(capture.hits(), 1);
        assert_eq!(
            state
                .app
                .usage_recorder
                .total_for("usage:key:vk_123:input_tokens"),
            0
        );
        assert_eq!(
            state
                .app
                .usage_recorder
                .total_for("usage:key:vk_123:output_tokens"),
            0
        );
    })
    .await;
}

fn test_state(snapshot: CompiledSnapshot) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), true, false),
        providers: ProviderRegistry::default(),
        admin: None,
    }
}

fn broken_redis_config(etcd: EtcdConfig) -> StartupConfig {
    StartupConfig {
        server: ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            admin_listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd,
        redis: RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: LogConfig {
            level: "info".to_string(),
        },
        runtime: RuntimeConfig { worker_threads: 1 },
        cache: CacheConfig {
            default: CacheDefaultMode::Disabled,
        },
        deployment: DeploymentConfig {
            admin: AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    }
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
        let lock = env_var_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

fn snapshot_with_inline_rpm_limit(base_url: &str, rpm: u64) -> CompiledSnapshot {
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
            allowed_models: vec!["gpt-4o-mini".to_string()],
        },
    );
    snapshot.key_limits.insert(
        "vk_123".to_string(),
        aisix_config::snapshot::ResolvedLimits {
            rpm: Some(rpm),
            tpm: None,
            concurrency: None,
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
            rate_limit: Some(RateLimitConfig {
                rpm: Some(rpm),
                tpm: None,
                concurrency: None,
            }),
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
            rate_limit: Some(RateLimitConfig {
                rpm: Some(rpm),
                tpm: None,
                concurrency: None,
            }),
            cache: None,
        },
    );

    snapshot
}

fn snapshot_with_mixed_scope_limits(base_url: &str) -> CompiledSnapshot {
    let mut snapshot = snapshot_with_inline_rpm_limit(base_url, 10);
    snapshot.provider_limits.insert(
        "openai".to_string(),
        aisix_config::snapshot::ResolvedLimits {
            rpm: None,
            tpm: Some(77),
            concurrency: Some(1),
        },
    );
    snapshot.model_limits.insert(
        "gpt-4o-mini".to_string(),
        aisix_config::snapshot::ResolvedLimits {
            rpm: None,
            tpm: Some(55),
            concurrency: None,
        },
    );
    snapshot.key_limits.insert(
        "vk_123".to_string(),
        aisix_config::snapshot::ResolvedLimits {
            rpm: Some(10),
            tpm: None,
            concurrency: None,
        },
    );

    snapshot
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
    hits: AtomicUsize,
}

impl CapturedRequest {
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

            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<Incoming>| {
                    let capture = capture.clone();
                    async move {
                        capture.hits.fetch_add(1, Ordering::SeqCst);

                        let _body = request.into_body().collect().await.unwrap().to_bytes();
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

#[derive(Default)]
struct BlockedUpstream {
    started: Notify,
    release: Notify,
}

impl BlockedUpstream {
    async fn wait_until_started(&self) {
        self.started.notified().await;
    }

    fn release(&self) {
        self.release.notify_waiters();
    }
}

async fn spawn_blocked_openai_mock(
    capture: Arc<CapturedRequest>,
    gate: Arc<BlockedUpstream>,
) -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let capture = capture.clone();
            let gate = gate.clone();

            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<Incoming>| {
                    let capture = capture.clone();
                    let gate = gate.clone();
                    async move {
                        capture.hits.fetch_add(1, Ordering::SeqCst);
                        let _body = request.into_body().collect().await.unwrap().to_bytes();
                        gate.started.notify_waiters();
                        gate.release.notified().await;

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

async fn spawn_error_openai_mock(capture: Arc<CapturedRequest>) -> MockUpstream {
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
                        let _body = request.into_body().collect().await.unwrap().to_bytes();

                        Ok::<_, std::convert::Infallible>(
                            hyper::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("content-type", "application/json")
                                .body(http_body_util::Full::new(Bytes::from_static(
                                    br#"{"error":{"message":"boom"}}"#,
                                )))
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

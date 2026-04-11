use std::sync::{Mutex, MutexGuard, OnceLock};

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
async fn anthropic_messages_stream_returns_expected_event_sequence() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::LfSuccessWithUsage).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app =
            aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));
        app.oneshot(anthropic_stream_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains("event: message_start"), "body was {text:?}");
    assert!(
        text.contains("event: content_block_start"),
        "body was {text:?}"
    );
    assert!(
        text.contains("event: content_block_delta"),
        "body was {text:?}"
    );
    assert!(text.contains("event: message_delta"), "body was {text:?}");
    assert!(text.contains("event: message_stop"), "body was {text:?}");
}

#[tokio::test]
async fn anthropic_messages_stream_converts_upstream_json_errors() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::JsonError).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app =
            aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));
        app.oneshot(anthropic_stream_request()).await.unwrap()
    })
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["type"], "authentication_error");
}

fn test_state(snapshot: CompiledSnapshot) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), true, false),
        providers: ProviderRegistry::default(),
        admin: None,
    }
}

fn snapshot_for_upstream(base_url: &str) -> CompiledSnapshot {
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

fn anthropic_stream_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "valid-token")
        .header("anthropic-version", "2023-06-01")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true,
                "max_tokens": 256
            }))
            .unwrap(),
        ))
        .unwrap()
}

struct MockUpstream {
    base_url: String,
}

#[derive(Clone, Copy)]
enum StreamMockResponse {
    LfSuccessWithUsage,
    JsonError,
}

async fn spawn_openai_stream_mock(response_kind: StreamMockResponse) -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let response_kind = response_kind;

            let service = service_fn(move |request: hyper::Request<Incoming>| async move {
                let body = request.into_body().collect().await.unwrap().to_bytes();
                let json: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(json["stream"], Value::Bool(true));

                let response = match response_kind {
                    StreamMockResponse::LfSuccessWithUsage => {
                        let response_body = concat!(
                            "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n",
                            "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                            "data: [DONE]\n\n"
                        );

                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(http_body_util::Full::new(Bytes::from_static(
                                response_body.as_bytes(),
                            )))
                            .unwrap()
                    }
                    StreamMockResponse::JsonError => {
                        let response_body = b"{ \n  \"error\": { \n    \"message\": \"bad upstream key\", \n    \"type\": \"authentication_error\" \n  }\n}";

                        hyper::Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .header("content-type", "application/json")
                            .body(http_body_util::Full::new(Bytes::from_static(response_body)))
                            .unwrap()
                    }
                };

                Ok::<_, std::convert::Infallible>(response)
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

use std::sync::{Mutex, MutexGuard, OnceLock};

use aisix_config::{
    etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind},
    snapshot::CompiledSnapshot,
    watcher::initial_snapshot_handle,
};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use aisix_types::{entities::KeyMeta, usage::Usage};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
};
use http_body_util::BodyExt;
use hyper::{body::{Bytes, Incoming}, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn stream_chat_returns_valid_openai_sse() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::LfSuccess).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));

        app.oneshot(stream_chat_request()).await.unwrap()
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
    assert_ne!(
        response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some("170")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.contains("data: {"), "body was {body:?}");
    assert!(body.trim_end().ends_with("data: [DONE]"), "body was {body:?}");
}

#[tokio::test]
async fn stream_chat_accepts_crlf_framed_upstream_sse() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::CrlfSuccess).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));

        app.oneshot(stream_chat_request()).await.unwrap()
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
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.contains("data: {"), "body was {body:?}");
    assert!(body.trim_end().ends_with("data: [DONE]"), "body was {body:?}");
}

#[tokio::test]
async fn stream_chat_preserves_json_error_responses() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::JsonError).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));

        app.oneshot(stream_chat_request()).await.unwrap()
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
    assert_ne!(
        response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some("74")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["message"], "bad upstream key");
}

#[tokio::test]
async fn stream_chat_records_and_exposes_usage_when_upstream_includes_it() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::LfSuccessWithUsage).await;
    let (state, response) = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let state = test_state(snapshot_for_upstream(&upstream.base_url));
        let app = aisix_server::app::build_router(state.clone());
        let response = app.oneshot(stream_chat_request()).await.unwrap();

        (state, response)
    })
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let usage = response
        .extensions()
        .get::<Usage>()
        .cloned()
        .expect("stream usage should be attached to response extensions");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 7);
    assert_eq!(state.app.usage_recorder.total_for("usage:key:vk_123:input_tokens"), 12);
    assert_eq!(state.app.usage_recorder.total_for("usage:key:vk_123:output_tokens"), 7);
}

#[tokio::test]
async fn stream_chat_forces_sse_content_type_on_successful_normalized_responses() {
    let upstream = spawn_openai_stream_mock(StreamMockResponse::WrongContentTypeSuccess).await;
    let response = with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_upstream(&upstream.base_url)));

        app.oneshot(stream_chat_request()).await.unwrap()
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
}

#[tokio::test]
async fn anthropic_stream_is_normalized_to_openai_sse() {
    let upstream = spawn_anthropic_stream_mock().await;
    let response = with_env_var("ANTHROPIC_API_KEY", Some("test-anthropic-key"), || async {
        let app = aisix_server::app::build_router(test_state(snapshot_for_anthropic_upstream(
            &upstream.base_url,
        )));

        app.oneshot(stream_chat_request()).await.unwrap()
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

    let usage = response
        .extensions()
        .get::<Usage>()
        .cloned()
        .expect("stream usage should be attached to response extensions");

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    let frames = sse_data_frames(&body);

    assert_eq!(frames.len(), 3, "body was {body:?}");

    let first: Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!(first["id"], "msg_test");
    assert_eq!(first["object"], "chat.completion.chunk");
    assert_eq!(first["choices"][0]["index"], 0);
    assert_eq!(first["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(first["choices"][0]["delta"]["content"], "hi");
    assert_eq!(first["choices"][0]["finish_reason"], Value::Null);

    let second: Value = serde_json::from_str(&frames[1]).unwrap();
    assert_eq!(second["id"], "msg_test");
    assert_eq!(second["object"], "chat.completion.chunk");
    assert_eq!(second["choices"][0]["delta"], json!({}));
    assert_eq!(second["choices"][0]["finish_reason"], "length");
    assert_eq!(second["usage"]["prompt_tokens"], 11);
    assert_eq!(second["usage"]["completion_tokens"], 3);
    assert_eq!(second["usage"]["total_tokens"], 14);

    assert_eq!(frames[2], "[DONE]", "body was {body:?}");
    assert_eq!(usage.input_tokens, 11);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(usage.cache_read, 5);
    assert_eq!(usage.cache_write, 7);
}

#[test]
fn rebuilt_non_success_stream_responses_strip_stale_body_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("content-length", HeaderValue::from_static("999"));
    headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));

    let response = aisix_server::stream_proxy::build_stream_response(
        StatusCode::BAD_GATEWAY,
        Body::from("{\"error\":\"upstream\"}"),
        headers,
        "openai",
        None,
    )
    .unwrap();

    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert!(response.headers().get("transfer-encoding").is_none());
    assert_ne!(
        response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some("999")
    );
}

fn test_state(snapshot: CompiledSnapshot) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), true),
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

fn snapshot_for_anthropic_upstream(base_url: &str) -> CompiledSnapshot {
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
        "anthropic".to_string(),
        ProviderConfig {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            base_url: base_url.to_string(),
            auth: ProviderAuth {
                secret_ref: "env:ANTHROPIC_API_KEY".to_string(),
            },
            policy_id: None,
            rate_limit: None,
        },
    );
    snapshot.models_by_name.insert(
        "gpt-4o-mini".to_string(),
        ModelConfig {
            id: "gpt-4o-mini".to_string(),
            provider_id: "anthropic".to_string(),
            upstream_model: "claude-3-5-haiku-latest".to_string(),
            policy_id: None,
            rate_limit: None,
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

fn sse_data_frames(body: &str) -> Vec<String> {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|frame| {
            let payload = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n");

            if payload.is_empty() {
                None
            } else {
                Some(payload)
            }
        })
        .collect()
}

fn stream_chat_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer valid-token")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "system", "content": "You are concise."},
                    {"role": "user", "content": "hello"}
                ],
                "stream": true
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
    LfSuccess,
    LfSuccessWithUsage,
    CrlfSuccess,
    WrongContentTypeSuccess,
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
                    StreamMockResponse::LfSuccess => {
                        let response_body = concat!(
                            "data: { \"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}] }\n\n",
                            "data: [DONE]\n\n"
                        );

                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .header("content-length", response_body.len().to_string())
                            .body(http_body_util::Full::new(Bytes::from_static(response_body.as_bytes())))
                            .unwrap()
                    }
                    StreamMockResponse::LfSuccessWithUsage => {
                        let response_body = concat!(
                            "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n",
                            "data: [DONE]\n\n"
                        );

                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .header("content-length", response_body.len().to_string())
                            .body(http_body_util::Full::new(Bytes::from_static(response_body.as_bytes())))
                            .unwrap()
                    }
                    StreamMockResponse::CrlfSuccess => {
                        let response_body = concat!(
                            "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\r\n\r\n",
                            "data: [DONE]\r\n\r\n"
                        );
                        let content_length = response_body.len().to_string();

                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .header("content-length", content_length)
                            .body(http_body_util::Full::new(Bytes::from_static(response_body.as_bytes())))
                            .unwrap()
                    }
                    StreamMockResponse::WrongContentTypeSuccess => {
                        let response_body = concat!(
                            "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
                            "data: [DONE]\n\n"
                        );

                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .header("content-length", response_body.len().to_string())
                            .body(http_body_util::Full::new(Bytes::from_static(response_body.as_bytes())))
                            .unwrap()
                    }
                    StreamMockResponse::JsonError => {
                        let response_body = b"{ \n  \"error\": { \n    \"message\": \"bad upstream key\", \n    \"type\": \"invalid_request_error\" \n  }\n}";

                        hyper::Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .header("content-type", "application/json")
                            .header("content-length", response_body.len().to_string())
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

async fn spawn_anthropic_stream_mock() -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            let service = service_fn(move |request: hyper::Request<Incoming>| async move {
                assert_eq!(request.uri().path(), "/v1/messages");
                assert_eq!(
                    request
                        .headers()
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok()),
                    Some("test-anthropic-key")
                );
                assert_eq!(
                    request
                        .headers()
                        .get("anthropic-version")
                        .and_then(|value| value.to_str().ok()),
                    Some("2023-06-01")
                );

                let body = request.into_body().collect().await.unwrap().to_bytes();
                let json: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(json["stream"], Value::Bool(true));
                assert_eq!(json["model"], Value::String("claude-3-5-haiku-latest".to_string()));
                assert_eq!(json["max_tokens"], 1024);
                assert_eq!(json["system"], Value::String("You are concise.".to_string()));
                assert_eq!(json["messages"], json!([
                    {"role": "user", "content": "hello"}
                ]));

                let response_body = concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":7}}}\n\n",
                    "event: content_block_delta\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
                    "event: message_delta\n",
                    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":3,\"cache_read_input_tokens\":5}}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                );

                let response = hyper::Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(http_body_util::Full::new(Bytes::from_static(
                        response_body.as_bytes(),
                    )))
                    .unwrap();

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

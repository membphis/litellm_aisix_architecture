use aisix_config::{snapshot::CompiledSnapshot, watcher::initial_snapshot_handle};
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn data_plane_handles_cors_preflight_for_chat() {
    let app = aisix_server::app::build_router(test_state());

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/v1/chat/completions")
                .header("origin", "http://127.0.0.1:4001")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization, content-type",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-methods")
            .and_then(|value| value.to_str().ok()),
        Some("GET,POST,OPTIONS")
    );
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-headers")
            .and_then(|value| value.to_str().ok()),
        Some("authorization,content-type,x-api-key,anthropic-version")
    );
}

fn test_state() -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(empty_snapshot()), true, false),
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

use aisix_cache::{CachedChatResponse, build_chat_cache_key};
use aisix_auth::extractor::AuthenticatedKey;
use aisix_policy::access::ensure_model_allowed;
use aisix_router::resolve::resolve_fixed_model;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
    usage::TransportMode,
};
use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Response},
};

use crate::{app::ServerState, stream_proxy::build_stream_response};

pub async fn run_json_pipeline(
    state: &ServerState,
    authenticated_key: &AuthenticatedKey,
    request: CanonicalRequest,
) -> Result<Response<Body>, GatewayError> {
    if request.transport_mode() != TransportMode::Json {
        return Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "streaming is not supported yet".to_string(),
        });
    }

    ensure_model_allowed(&authenticated_key.meta, request.model_name())?;

    let snapshot = state.app.snapshot.load();
    let target = resolve_fixed_model(&snapshot, request.model_name())?;
    let provider = snapshot
        .providers_by_id
        .get(&target.provider_id)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("provider '{}' not found", target.provider_id),
        })?;

    let _concurrency_guard = state
        .app
        .rate_limits
        .precheck(
            &snapshot,
            &authenticated_key.meta.key_id,
            request.model_name(),
            &provider.id,
        )
        .await?;

    let codec = state.providers.resolve(provider)?;

    if let CanonicalRequest::Chat(chat_request) = &request {
        let cache_key = build_chat_cache_key(
            snapshot.revision,
            &provider.id,
            &target.upstream_model,
            &chat_request.model,
            &chat_request.messages,
        )
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build chat cache key: {error}"),
        })?;

        if let Some(cached) = state.app.cache.get_chat(&cache_key) {
            if let Some(usage) = cached.usage.as_ref() {
                state
                    .app
                    .usage_recorder
                    .record_success(&authenticated_key.meta, request.model_name(), usage)
                    .await;
            }

            return build_response(
                http::StatusCode::OK,
                cached.body,
                http::HeaderMap::new(),
                Some("true"),
                Some(&cached.provider_id),
                cached.usage,
            );
        }

        let output = codec
            .execute_json(provider, &target.upstream_model, &request)
            .await?;
        let usage = output.usage.clone();

        if output.status.is_success() {
            state.app.cache.put_chat(
                cache_key,
                CachedChatResponse {
                    body: output.body.clone(),
                    provider_id: provider.id.clone(),
                    usage: usage.clone(),
                },
            );
            if let Some(usage) = usage.as_ref() {
                state
                    .app
                    .usage_recorder
                    .record_success(&authenticated_key.meta, request.model_name(), usage)
                    .await;
            }
        }

        return build_response(output.status, output.body, output.headers, Some("false"), Some(&provider.id), usage);
    }

    let output = codec
        .execute_json(provider, &target.upstream_model, &request)
        .await?;
    let usage = output.usage.clone();

    if output.status.is_success() {
        if let Some(usage) = usage.as_ref() {
            state
                .app
                .usage_recorder
                .record_success(&authenticated_key.meta, request.model_name(), usage)
                .await;
        }
    }

    build_response(output.status, output.body, output.headers, None, Some(&provider.id), usage)
}

pub async fn run_chat_stream_pipeline(
    state: &ServerState,
    authenticated_key: &AuthenticatedKey,
    request: CanonicalRequest,
) -> Result<Response<Body>, GatewayError> {
    if request.transport_mode() != TransportMode::SseStream {
        return Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "expected streaming chat request".to_string(),
        });
    }

    ensure_model_allowed(&authenticated_key.meta, request.model_name())?;

    let snapshot = state.app.snapshot.load();
    let target = resolve_fixed_model(&snapshot, request.model_name())?;
    let provider = snapshot
        .providers_by_id
        .get(&target.provider_id)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("provider '{}' not found", target.provider_id),
        })?;

    let _concurrency_guard = state
        .app
        .rate_limits
        .precheck(
            &snapshot,
            &authenticated_key.meta.key_id,
            request.model_name(),
            &provider.id,
        )
        .await?;

    let codec = state.providers.resolve(provider)?;
    let output = codec
        .execute_stream(provider, &target.upstream_model, &request)
        .await?;
    let usage = output.usage.clone();

    if output.status.is_success() {
        if let Some(usage) = usage.as_ref() {
            state
                .app
                .usage_recorder
                .record_success(&authenticated_key.meta, request.model_name(), usage)
                .await;
        }
    }

    build_stream_response(output.status, output.body, output.headers, &provider.id, usage)
}

fn build_response(
    status: http::StatusCode,
    body: impl Into<Body>,
    mut headers: http::HeaderMap,
    cache_hit: Option<&str>,
    provider_id: Option<&str>,
    usage: Option<aisix_types::usage::Usage>,
) -> Result<Response<Body>, GatewayError> {
    if cache_hit.is_some() && !headers.contains_key(http::header::CONTENT_TYPE) {
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }

    let mut response = Response::builder()
        .status(status)
        .body(body.into())
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build response: {error}"),
        })?;

    response.headers_mut().extend(headers);

    if let Some(cache_hit) = cache_hit {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-cache-hit"),
            HeaderValue::from_str(cache_hit).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set cache header: {error}"),
            })?,
        );
    }

    if let Some(provider_id) = provider_id {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-provider"),
            HeaderValue::from_str(provider_id).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set provider header: {error}"),
            })?,
        );
    }

    if let Some(usage) = usage {
        response.extensions_mut().insert(usage);
    }

    Ok(response)
}

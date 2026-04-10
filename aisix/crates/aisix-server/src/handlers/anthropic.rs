use aisix_core::RequestContext;
use aisix_types::{
    anthropic::AnthropicMessagesRequest,
    entities::KeyMeta,
    error::{ErrorKind, GatewayError},
    request::{CanonicalRequest, ProtocolFamily, TransportMode},
};
use axum::{
    body::Bytes,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap},
    response::Response,
};
use chrono::Utc;

use crate::{
    app::ServerState,
    pipeline::{authorization, cache, post_call, rate_limit, route_select, stream_chunk},
    protocol,
};

pub async fn messages(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match run_messages(state, headers, body).await {
        Ok(response) => response,
        Err(error) => protocol::anthropic::render_gateway_error(error),
    }
}

async fn run_messages(
    state: ServerState,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let version = protocol::anthropic::require_anthropic_version(&headers)?;
    let request: AnthropicMessagesRequest = serde_json::from_slice(&body).map_err(|error| {
        GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: format!("invalid anthropic request body: {error}"),
        }
    })?;
    let snapshot = state.app.snapshot.load_full();
    let key_meta = authenticate_key(&headers, &snapshot.keys_by_token)?;
    let canonical = protocol::anthropic::into_canonical_request(request)?;
    let mut ctx = RequestContext::new(CanonicalRequest::Chat(canonical), key_meta, snapshot);
    ctx.ingress_protocol = ProtocolFamily::Anthropic;
    ctx.egress_protocol = ProtocolFamily::Anthropic;
    ctx.anthropic_version = Some(version);

    authorization::check(&ctx)?;
    route_select::resolve(&mut ctx, &state)?;
    let _rate_limit_guard = rate_limit::check(&ctx, &state).await?;

    if ctx.request.transport_mode() == TransportMode::Json && cache::cache_enabled_for_chat(&ctx, &state)? {
        if let Some(response) = cache::lookup_chat(&mut ctx, &state)? {
            post_call::record_success(&ctx, &state).await;
            return Ok(response);
        }
    }

    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    if response.status().is_success() {
        post_call::record_success(&ctx, &state).await;
    }
    Ok(response)
}

fn authenticate_key(
    headers: &HeaderMap,
    keys_by_token: &std::collections::HashMap<String, KeyMeta>,
) -> Result<KeyMeta, GatewayError> {
    let token = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| bearer_token(headers).ok())
        .ok_or_else(invalid_api_key)?;

    let meta = keys_by_token.get(&token).cloned().ok_or_else(invalid_api_key)?;
    if meta.expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) {
        return Err(invalid_api_key());
    }

    Ok(meta)
}

fn bearer_token(headers: &HeaderMap) -> Result<String, GatewayError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(invalid_api_key)?;
    let (scheme, token) = value.split_once(' ').ok_or_else(invalid_api_key)?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(invalid_api_key());
    }

    Ok(token.to_string())
}

fn invalid_api_key() -> GatewayError {
    GatewayError {
        kind: ErrorKind::Authentication,
        message: "Invalid API key".to_string(),
    }
}

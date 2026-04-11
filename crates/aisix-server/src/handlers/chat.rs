use aisix_auth::extractor::AuthenticatedKey;
use aisix_core::RequestContext;
use aisix_types::{error::GatewayError, request::ChatRequest, usage::TransportMode};
use axum::{extract::State, response::Response, Json};

use crate::{
    app::ServerState,
    pipeline::{authorization, cache, post_call, rate_limit, route_select, stream_chunk},
};

pub async fn chat_completions(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<ChatRequest>,
) -> Result<Response, GatewayError> {
    let snapshot = state.app.snapshot.load_full();
    let mut ctx = RequestContext::new(
        aisix_types::request::CanonicalRequest::Chat(request.into_canonical()?),
        authenticated_key.meta,
        snapshot,
    );

    authorization::check(&ctx)?;
    route_select::resolve(&mut ctx, &state)?;
    let _rate_limit_guard = rate_limit::check(&ctx, &state).await?;

    if ctx.request.transport_mode() == TransportMode::Json
        && cache::cache_enabled_for_chat(&ctx, &state)?
    {
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

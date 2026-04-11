use aisix_auth::extractor::AuthenticatedKey;
use aisix_core::RequestContext;
use aisix_types::{
    error::GatewayError,
    request::{CanonicalRequest, EmbeddingsRequest},
};
use axum::{extract::State, response::Response, Json};

use crate::{
    app::ServerState,
    pipeline::{authorization, post_call, rate_limit, route_select, stream_chunk},
};

pub async fn embeddings(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, GatewayError> {
    let snapshot = state.app.snapshot.load_full();
    let mut ctx = RequestContext::new(
        CanonicalRequest::Embeddings(request),
        authenticated_key.meta,
        snapshot,
    );

    authorization::check(&ctx)?;
    route_select::resolve(&mut ctx, &state)?;
    let _rate_limit_guard = rate_limit::check(&ctx, &state).await?;

    let response = stream_chunk::proxy(&mut ctx, &state).await?;
    if response.status().is_success() {
        post_call::record_success(&ctx, &state).await;
    }

    Ok(response)
}

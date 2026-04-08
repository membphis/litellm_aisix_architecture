use aisix_auth::extractor::AuthenticatedKey;
use aisix_types::{
    error::GatewayError,
    request::{CanonicalRequest, EmbeddingsRequest},
};
use axum::{
    Json,
    extract::State,
    response::Response,
};

use crate::{app::ServerState, pipeline::run_json_pipeline};

pub async fn embeddings(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, GatewayError> {
    run_json_pipeline(&state, &authenticated_key, CanonicalRequest::Embeddings(request)).await
}

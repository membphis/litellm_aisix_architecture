use aisix_auth::extractor::AuthenticatedKey;
use aisix_types::{
    error::GatewayError,
    request::{CanonicalRequest, ChatRequest},
};
use axum::{
    Json,
    extract::State,
    response::Response,
};

use crate::{app::ServerState, pipeline::{run_chat_stream_pipeline, run_json_pipeline}};

pub async fn chat_completions(
    State(state): State<ServerState>,
    authenticated_key: AuthenticatedKey,
    Json(request): Json<ChatRequest>,
) -> Result<Response, GatewayError> {
    let request = CanonicalRequest::Chat(request);

    if request.transport_mode() == aisix_types::usage::TransportMode::SseStream {
        return run_chat_stream_pipeline(&state, &authenticated_key, request).await;
    }

    run_json_pipeline(&state, &authenticated_key, request).await
}

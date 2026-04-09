use aisix_core::RequestContext;
use aisix_types::error::{ErrorKind, GatewayError};
use aisix_types::{request::CanonicalRequest, usage::TransportMode};
use axum::{body::Body, http::Response};

use crate::{
    app::ServerState,
    pipeline::{cache, response::build_json_response},
    stream_proxy::build_stream_response,
};

pub async fn proxy(
    ctx: &mut RequestContext,
    state: &ServerState,
) -> Result<Response<Body>, GatewayError> {
    let provider_id = ctx
        .resolved_provider_id
        .as_deref()
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved provider missing before upstream execution".to_string(),
        })?;
    let upstream_model = ctx
        .resolved_target
        .as_ref()
        .map(|target| target.upstream_model.as_str())
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved target missing before upstream execution".to_string(),
        })?;
    let provider = ctx
        .snapshot
        .providers_by_id
        .get(provider_id)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("provider '{}' not found", provider_id),
        })?;
    let codec = state.providers.resolve(provider)?;

    match ctx.request.transport_mode() {
        TransportMode::Json => {
            let output = codec
                .execute_json(provider, upstream_model, &ctx.request)
                .await?;

            if output.status.is_success() {
                ctx.usage = output.usage.clone();
                if matches!(&ctx.request, CanonicalRequest::Chat(chat_request) if !chat_request.stream)
                {
                    cache::store_chat_success(
                        ctx,
                        state,
                        output.body.as_ref(),
                        output.usage.clone(),
                    )?;
                }
            }

            let cache_hit = match &ctx.request {
                CanonicalRequest::Chat(_) => Some("false"),
                CanonicalRequest::Embeddings(_) => None,
            };

            build_json_response(
                output.status,
                output.body,
                output.headers,
                cache_hit,
                Some(provider_id),
                output.usage,
            )
        }
        TransportMode::SseStream => {
            let output = codec
                .execute_stream(provider, upstream_model, &ctx.request)
                .await?;
            if output.status.is_success() {
                ctx.usage = output.usage.clone();
            }

            build_stream_response(
                output.status,
                output.body,
                output.headers,
                provider_id,
                output.usage,
            )
        }
    }
}

use aisix_core::RequestContext;
use aisix_types::error::{ErrorKind, GatewayError};
use aisix_types::{
    request::{CanonicalRequest, ProtocolFamily},
    usage::TransportMode,
};
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
            message: format!("provider '{provider_id}' not found"),
        })?;
    let codec = state.providers.resolve(provider)?;

    match ctx.request.transport_mode() {
        TransportMode::Json => {
            let cache_enabled = cache::cache_enabled_for_chat(ctx, state)?;
            let output = codec
                .execute_json(provider, upstream_model, &ctx.request)
                .await?;

            if output.status.is_success() {
                ctx.usage = output.usage.clone();
                if cache_enabled
                    && matches!(&ctx.request, CanonicalRequest::Chat(chat_request) if !chat_request.stream)
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
                CanonicalRequest::Chat(_) if cache_enabled => Some("false"),
                CanonicalRequest::Chat(_) => None,
                CanonicalRequest::Embeddings(_) => None,
            };

            match ctx.egress_protocol {
                ProtocolFamily::OpenAi => build_json_response(
                    output.status,
                    output.body,
                    output.headers,
                    cache_hit,
                    Some(provider_id),
                    output.usage,
                ),
                ProtocolFamily::Anthropic => {
                    crate::protocol::anthropic::build_anthropic_json_response(
                        output.status,
                        output.body.as_ref(),
                        output.usage,
                        ctx.request.model_name(),
                    )
                }
            }
        }
        TransportMode::SseStream => {
            let output = codec
                .execute_stream(provider, upstream_model, &ctx.request)
                .await?;
            if output.status.is_success() {
                ctx.usage = output.usage.clone();
            }

            match ctx.egress_protocol {
                ProtocolFamily::OpenAi => build_stream_response(
                    output.status,
                    output.body,
                    output.headers,
                    provider_id,
                    output.usage,
                ),
                ProtocolFamily::Anthropic => {
                    crate::protocol::anthropic::build_anthropic_stream_proxy_response(
                        output.status,
                        output.body,
                        output.headers,
                        provider_id,
                        output.usage,
                        ctx.request.model_name(),
                    )
                }
            }
        }
    }
}

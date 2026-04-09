use aisix_cache::{build_chat_cache_key, CachedChatResponse};
use aisix_core::RequestContext;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
    usage::Usage,
};
use axum::{
    body::{Body, Bytes},
    http::Response,
};

use crate::{app::ServerState, pipeline::response::build_json_response};

fn cache_key_for_chat(ctx: &RequestContext) -> Result<Option<String>, GatewayError> {
    let CanonicalRequest::Chat(chat_request) = &ctx.request else {
        return Ok(None);
    };

    let provider_id = ctx
        .resolved_provider_id
        .as_deref()
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved provider missing before cache lookup".to_string(),
        })?;
    let upstream_model = ctx
        .resolved_target
        .as_ref()
        .map(|target| target.upstream_model.as_str())
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved target missing before cache lookup".to_string(),
        })?;
    build_chat_cache_key(
        ctx.snapshot.revision,
        provider_id,
        upstream_model,
        &chat_request.model,
        &chat_request.messages,
    )
    .map(Some)
    .map_err(|error| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("failed to build chat cache key: {error}"),
    })
}

pub fn lookup_chat(
    ctx: &mut RequestContext,
    state: &ServerState,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(cache_key) = cache_key_for_chat(ctx)? else {
        return Ok(None);
    };

    let Some(cached) = state.app.cache.get_chat(&cache_key) else {
        return Ok(None);
    };

    ctx.response_cached = true;
    ctx.usage = cached.usage.clone();

    Ok(Some(build_json_response(
        http::StatusCode::OK,
        cached.body,
        http::HeaderMap::new(),
        Some("true"),
        Some(&cached.provider_id),
        cached.usage,
    )?))
}

pub fn store_chat_success(
    ctx: &RequestContext,
    state: &ServerState,
    response_body: &[u8],
    usage: Option<Usage>,
) -> Result<(), GatewayError> {
    let CanonicalRequest::Chat(chat_request) = &ctx.request else {
        return Ok(());
    };

    if chat_request.stream {
        return Ok(());
    }

    let Some(cache_key) = cache_key_for_chat(ctx)? else {
        return Ok(());
    };
    let provider_id = ctx
        .resolved_provider_id
        .clone()
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved provider missing before cache store".to_string(),
        })?;

    state.app.cache.put_chat(
        cache_key,
        CachedChatResponse {
            body: Bytes::copy_from_slice(response_body),
            provider_id,
            usage,
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use aisix_cache::{build_chat_cache_key, CachedChatResponse};
    use aisix_config::{
        etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind},
        snapshot::CompiledSnapshot,
        watcher::initial_snapshot_handle,
    };
    use aisix_core::{AppState, RequestContext};
    use aisix_providers::ProviderRegistry;
    use aisix_types::{
        entities::KeyMeta,
        request::{CanonicalRequest, ChatRequest},
    };
    use axum::body::Bytes;
    use serde_json::json;

    use super::lookup_chat;
    use crate::{app::ServerState, pipeline::route_select};

    #[test]
    fn lookup_chat_uses_request_snapshot_after_route_selection() {
        let snapshot_v1 = snapshot_for_cache(1, "openai", "gpt-4o-mini-2024-07-18");
        let state = ServerState {
            app: AppState::new(initial_snapshot_handle(snapshot_v1.clone()), true),
            providers: ProviderRegistry::default(),
            admin: None,
        };
        let mut ctx = RequestContext::new(
            CanonicalRequest::Chat(ChatRequest {
                model: "gpt-4o-mini".to_string(),
                messages: vec![json!({"role": "user", "content": "hello"})],
                stream: false,
            }),
            KeyMeta {
                key_id: "vk_123".to_string(),
                user_id: None,
                customer_id: None,
                alias: Some("test-key".to_string()),
                expires_at: None,
                allowed_models: vec!["gpt-4o-mini".to_string()],
            },
            std::sync::Arc::new(snapshot_v1.clone()),
        );

        route_select::resolve(&mut ctx, &state).unwrap();

        let cache_key = build_chat_cache_key(
            snapshot_v1.revision,
            "openai",
            "gpt-4o-mini-2024-07-18",
            "gpt-4o-mini",
            &[json!({"role": "user", "content": "hello"})],
        )
        .unwrap();
        state.app.cache.put_chat(
            cache_key,
            CachedChatResponse {
                body: Bytes::from_static(br#"{"id":"cached"}"#),
                provider_id: "openai".to_string(),
                usage: None,
            },
        );

        state
            .app
            .snapshot
            .store(std::sync::Arc::new(snapshot_for_cache(
                2,
                "openai-v2",
                "gpt-4o-mini-2024-08-01",
            )));

        let cached = lookup_chat(&mut ctx, &state).unwrap();

        assert!(
            cached.is_some(),
            "cache lookup should use the original request snapshot"
        );
    }

    fn snapshot_for_cache(
        revision: i64,
        provider_id: &str,
        upstream_model: &str,
    ) -> CompiledSnapshot {
        let mut snapshot = CompiledSnapshot {
            revision,
            keys_by_token: Default::default(),
            apikeys_by_id: Default::default(),
            providers_by_id: Default::default(),
            models_by_name: Default::default(),
            policies_by_id: Default::default(),
            provider_limits: Default::default(),
            model_limits: Default::default(),
            key_limits: Default::default(),
        };

        snapshot.providers_by_id.insert(
            provider_id.to_string(),
            ProviderConfig {
                id: provider_id.to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://example.invalid".to_string(),
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
                provider_id: provider_id.to_string(),
                upstream_model: upstream_model.to_string(),
                policy_id: None,
                rate_limit: None,
            },
        );

        snapshot
    }
}

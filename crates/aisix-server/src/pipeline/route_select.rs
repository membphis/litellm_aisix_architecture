use aisix_core::{context::ResolvedTarget, RequestContext};
use aisix_router::resolve::resolve_fixed_model;
use aisix_types::error::{ErrorKind, GatewayError};

use crate::app::ServerState;

pub fn resolve(ctx: &mut RequestContext, _state: &ServerState) -> Result<(), GatewayError> {
    let target = resolve_fixed_model(ctx.snapshot.as_ref(), ctx.request.model_name())?;
    let provider = ctx
        .snapshot
        .providers_by_id
        .get(&target.provider_id)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("provider '{}' not found", target.provider_id),
        })?;

    ctx.resolved_provider_id = Some(provider.id.clone());
    ctx.resolved_target = Some(ResolvedTarget {
        upstream_model: target.upstream_model.clone(),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use aisix_config::{
        etcd_model::{ModelConfig, ProviderAuth, ProviderConfig, ProviderKind},
        snapshot::CompiledSnapshot,
        watcher::initial_snapshot_handle,
    };
    use aisix_core::{AppState, RequestContext};
    use aisix_providers::ProviderRegistry;
    use aisix_types::{
        entities::KeyMeta,
        request::{CanonicalRequest, EmbeddingsRequest},
    };
    use serde_json::json;

    use super::resolve;
    use crate::app::ServerState;

    #[test]
    fn resolve_populates_provider_and_upstream_model_on_context() {
        let snapshot = snapshot_for_route("https://example.invalid");
        let state = ServerState {
            app: AppState::new(initial_snapshot_handle(snapshot.clone()), true, false),
            providers: ProviderRegistry::default(),
            admin: None,
        };
        let mut context = RequestContext::new(
            CanonicalRequest::Embeddings(EmbeddingsRequest {
                model: "text-embedding-3-small".to_string(),
                input: json!("hello"),
            }),
            KeyMeta {
                key_id: "vk_123".to_string(),
                user_id: None,
                customer_id: None,
                alias: Some("test-key".to_string()),
                expires_at: None,
                allowed_models: vec!["text-embedding-3-small".to_string()],
            },
            std::sync::Arc::new(snapshot),
        );

        resolve(&mut context, &state).unwrap();

        assert_eq!(context.resolved_provider_id.as_deref(), Some("openai"));
        assert_eq!(
            context
                .resolved_target
                .as_ref()
                .map(|target| target.upstream_model.as_str()),
            Some("text-embedding-3-small-upstream")
        );
    }

    fn snapshot_for_route(base_url: &str) -> CompiledSnapshot {
        let mut snapshot = CompiledSnapshot {
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
        };

        snapshot.providers_by_id.insert(
            "openai".to_string(),
            ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: base_url.to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            },
        );
        snapshot.models_by_name.insert(
            "text-embedding-3-small".to_string(),
            ModelConfig {
                id: "text-embedding-3-small".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "text-embedding-3-small-upstream".to_string(),
                policy_id: None,
                rate_limit: None,
                cache: None,
            },
        );

        snapshot
    }
}

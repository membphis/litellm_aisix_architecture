use std::sync::Arc;

use aisix_config::snapshot::CompiledSnapshot;
use uuid::Uuid;

use aisix_types::{entities::KeyMeta, request::CanonicalRequest, usage::Usage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub upstream_model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestContext {
    pub request_id: Uuid,
    pub request: CanonicalRequest,
    pub key_meta: KeyMeta,
    pub snapshot: Arc<CompiledSnapshot>,
    pub resolved_target: Option<ResolvedTarget>,
    pub resolved_provider_id: Option<String>,
    pub usage: Option<Usage>,
    pub response_cached: bool,
}

impl RequestContext {
    pub fn new(
        request: CanonicalRequest,
        key_meta: KeyMeta,
        snapshot: Arc<CompiledSnapshot>,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            request,
            key_meta,
            snapshot,
            resolved_target: None,
            resolved_provider_id: None,
            usage: None,
            response_cached: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestContext;
    use aisix_types::{
        entities::KeyMeta,
        request::{CanonicalRequest, EmbeddingsRequest},
    };
    use serde_json::json;

    use aisix_config::snapshot::CompiledSnapshot;
    use std::sync::Arc;

    #[test]
    fn new_context_starts_without_resolved_route_or_usage() {
        let request = CanonicalRequest::Embeddings(EmbeddingsRequest {
            model: "text-embedding-3-small".to_string(),
            input: json!("hello"),
        });
        let key_meta = KeyMeta {
            key_id: "vk_123".to_string(),
            user_id: None,
            customer_id: None,
            alias: Some("test-key".to_string()),
            expires_at: None,
            allowed_models: vec!["text-embedding-3-small".to_string()],
        };

        let context = RequestContext::new(
            request,
            key_meta,
            Arc::new(CompiledSnapshot {
                revision: 0,
                keys_by_token: Default::default(),
                apikeys_by_id: Default::default(),
                providers_by_id: Default::default(),
                models_by_name: Default::default(),
                policies_by_id: Default::default(),
                provider_limits: Default::default(),
                model_limits: Default::default(),
                key_limits: Default::default(),
            }),
        );

        assert!(context.resolved_target.is_none());
        assert!(context.resolved_provider_id.is_none());
        assert!(context.usage.is_none());
        assert!(!context.response_cached);
    }
}

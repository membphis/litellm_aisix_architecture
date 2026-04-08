use uuid::Uuid;

use aisix_types::{entities::KeyMeta, request::CanonicalRequest, usage::Usage};

#[derive(Debug, Clone, PartialEq)]
pub struct RequestContext {
    pub request_id: Uuid,
    pub request: CanonicalRequest,
    pub key_meta: KeyMeta,
    pub selected_provider_id: String,
    pub usage: Usage,
    pub cache_hit: bool,
}

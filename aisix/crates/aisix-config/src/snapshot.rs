use std::collections::HashMap;

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig, RateLimitConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLimits {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
    pub concurrency: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSnapshot {
    pub revision: i64,
    pub keys_by_token: HashMap<String, KeyMeta>,
    pub apikeys_by_id: HashMap<String, ApiKeyConfig>,
    pub providers_by_id: HashMap<String, ProviderConfig>,
    pub models_by_name: HashMap<String, ModelConfig>,
    pub policies_by_id: HashMap<String, PolicyConfig>,
    pub provider_limits: HashMap<String, ResolvedLimits>,
    pub model_limits: HashMap<String, ResolvedLimits>,
    pub key_limits: HashMap<String, ResolvedLimits>,
}

impl From<&RateLimitConfig> for ResolvedLimits {
    fn from(value: &RateLimitConfig) -> Self {
        Self {
            rpm: value.rpm,
            tpm: value.tpm,
            concurrency: value.concurrency,
        }
    }
}

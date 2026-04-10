use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
    pub concurrency: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub id: String,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    pub id: String,
    pub key: String,
    pub allowed_models: Vec<String>,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheMode {
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachePolicyConfig {
    pub mode: CacheMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
    pub cache: Option<CachePolicyConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth: ProviderAuth,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
    pub cache: Option<CachePolicyConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAuth {
    pub secret_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "azure_openai")]
    AzureOpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
}

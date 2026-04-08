use aisix_config::etcd_model::{ProviderConfig, ProviderKind};
use aisix_types::error::GatewayError;

use crate::{anthropic::AnthropicCodec, codec::ProviderCodec, openai_compat::OpenAiCompatCodec};

#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    anthropic: AnthropicCodec,
    openai_compat: OpenAiCompatCodec,
}

impl ProviderRegistry {
    pub fn resolve(&self, provider: &ProviderConfig) -> Result<&dyn ProviderCodec, GatewayError> {
        let codec: &dyn ProviderCodec = match provider.kind {
            ProviderKind::OpenAi | ProviderKind::AzureOpenAi => &self.openai_compat,
            ProviderKind::Anthropic => &self.anthropic,
        };

        Ok(codec)
    }
}

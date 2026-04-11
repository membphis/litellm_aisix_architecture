use aisix_config::etcd_model::ProviderConfig;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
    usage::Usage,
};
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};

#[derive(Debug, Clone)]
pub struct JsonOutput {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone)]
pub struct StreamOutput {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub usage: Option<Usage>,
}

#[async_trait]
pub trait ProviderCodec: Send + Sync {
    fn provider_id(&self) -> &'static str;

    async fn execute_json(
        &self,
        provider: &ProviderConfig,
        upstream_model: &str,
        request: &CanonicalRequest,
    ) -> Result<JsonOutput, GatewayError>;

    async fn execute_stream(
        &self,
        _provider: &ProviderConfig,
        _upstream_model: &str,
        _request: &CanonicalRequest,
    ) -> Result<StreamOutput, GatewayError> {
        Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: format!("{} does not support streaming", self.provider_id()),
        })
    }
}

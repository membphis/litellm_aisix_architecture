use aisix_config::etcd_model::{ProviderConfig, ProviderKind};
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
    usage::Usage,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

use crate::{
    codec::{JsonOutput, ProviderCodec, StreamOutput},
    openai_sse::{OpenAiSseEvent, normalize_openai_chat_sse},
};

#[derive(Debug, Clone, Default)]
pub struct OpenAiCompatCodec {
    client: reqwest::Client,
}

#[async_trait]
impl ProviderCodec for OpenAiCompatCodec {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    async fn execute_json(
        &self,
        provider: &ProviderConfig,
        upstream_model: &str,
        request: &CanonicalRequest,
    ) -> Result<JsonOutput, GatewayError> {
        let secret = resolve_secret(&provider.auth.secret_ref)?;
        let (path, body) = build_upstream_request(request, upstream_model)?;
        let url = format!("{}{path}", provider.base_url.trim_end_matches('/'));
        let request_builder = apply_auth(self.client.post(url), provider, &secret);
        let response = request_builder
            .json(&body)
            .send()
            .await
            .map_err(|error| GatewayError {
                kind: ErrorKind::Upstream,
                message: format!("upstream request failed: {error}"),
            })?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(|error| GatewayError {
            kind: ErrorKind::Upstream,
            message: format!("failed to read upstream response: {error}"),
        })?;
        let usage = extract_usage(&body);

        Ok(JsonOutput {
            status,
            headers,
            body,
            usage,
        })
    }

    async fn execute_stream(
        &self,
        provider: &ProviderConfig,
        upstream_model: &str,
        request: &CanonicalRequest,
    ) -> Result<StreamOutput, GatewayError> {
        let secret = resolve_secret(&provider.auth.secret_ref)?;
        let (path, body) = build_upstream_request(request, upstream_model)?;
        let url = format!("{}{path}", provider.base_url.trim_end_matches('/'));
        let request_builder = apply_auth(self.client.post(url), provider, &secret);
        let response = request_builder
            .json(&body)
            .send()
            .await
            .map_err(|error| GatewayError {
                kind: ErrorKind::Upstream,
                message: format!("upstream request failed: {error}"),
            })?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(|error| GatewayError {
            kind: ErrorKind::Upstream,
            message: format!("failed to read upstream response: {error}"),
        })?;

        if !status.is_success() {
            return Ok(StreamOutput {
                status,
                headers,
                body,
                usage: None,
            });
        }

        let normalized = normalize_openai_chat_sse(&body)?;
        let usage = normalized.usage.clone();
        let body = render_normalized_events(&normalized.events);
        let headers = strip_unsafe_body_headers(headers);

        Ok(StreamOutput {
            status,
            headers,
            body,
            usage,
        })
    }
}

fn render_normalized_events(events: &[OpenAiSseEvent]) -> Bytes {
    let mut rendered = Vec::new();

    for event in events {
        match event {
            OpenAiSseEvent::Data(data) => {
                rendered.extend_from_slice(b"data: ");
                rendered.extend_from_slice(data);
                rendered.extend_from_slice(b"\n\n");
            }
            OpenAiSseEvent::Done => rendered.extend_from_slice(b"data: [DONE]\n\n"),
        }
    }

    Bytes::from(rendered)
}

fn strip_unsafe_body_headers(mut headers: reqwest::header::HeaderMap) -> reqwest::header::HeaderMap {
    headers.remove(reqwest::header::CONTENT_LENGTH);
    headers.remove(reqwest::header::TRANSFER_ENCODING);
    headers
}

fn build_upstream_request(
    request: &CanonicalRequest,
    upstream_model: &str,
) -> Result<(&'static str, serde_json::Value), GatewayError> {
    match request {
        CanonicalRequest::Chat(request) => {
            let mut upstream_request = request.clone();
            upstream_request.model = upstream_model.to_string();
            let body = serde_json::to_value(upstream_request).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize chat request: {error}"),
            })?;

            Ok(("/v1/chat/completions", body))
        }
        CanonicalRequest::Embeddings(request) => {
            let mut upstream_request = request.clone();
            upstream_request.model = upstream_model.to_string();
            let body = serde_json::to_value(upstream_request).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize embeddings request: {error}"),
            })?;

            Ok(("/v1/embeddings", body))
        }
    }
}

fn apply_auth(
    request_builder: reqwest::RequestBuilder,
    provider: &ProviderConfig,
    secret: &str,
) -> reqwest::RequestBuilder {
    match provider.kind {
        ProviderKind::AzureOpenAi => request_builder.header("api-key", secret),
        _ => request_builder.bearer_auth(secret),
    }
}

fn resolve_secret(secret_ref: &str) -> Result<String, GatewayError> {
    let Some(env_name) = secret_ref.strip_prefix("env:") else {
        return Err(GatewayError {
            kind: ErrorKind::Internal,
            message: format!("unsupported provider secret ref: {secret_ref}"),
        });
    };

    std::env::var(env_name).map_err(|_| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("missing provider secret env var: {env_name}"),
    })
}

fn extract_usage(body: &[u8]) -> Option<Usage> {
    let json: Value = serde_json::from_slice(body).ok()?;
    let usage = json.get("usage")?;

    Some(Usage {
        input_tokens: usage.get("prompt_tokens")?.as_u64()?,
        output_tokens: usage
            .get("completion_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        cache_read: 0,
        cache_write: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::extract_usage;

    #[test]
    fn embeddings_usage_defaults_missing_completion_tokens_to_zero() {
        let body = br#"{
            "object": "list",
            "usage": {
                "prompt_tokens": 3,
                "total_tokens": 3
            }
        }"#;

        let usage = extract_usage(body).expect("usage should be extracted");

        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 0);
    }
}

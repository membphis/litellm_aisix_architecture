use aisix_config::etcd_model::{ProviderConfig, ProviderKind};
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::{CanonicalContentPart, CanonicalRequest, CanonicalRole},
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
            if let Some(raw_messages) = &request.raw_messages {
                let mut body = serde_json::json!({
                    "model": upstream_model,
                    "messages": raw_messages,
                    "stream": request.stream,
                });

                if let Some(max_tokens) = request.max_tokens {
                    body["max_tokens"] = serde_json::json!(max_tokens);
                }
                if !request.stop_sequences.is_empty() {
                    body["stop"] = serde_json::json!(request.stop_sequences);
                }
                if let Some(temperature) = request.temperature {
                    body["temperature"] = serde_json::json!(temperature);
                }
                if let Some(top_p) = request.top_p {
                    body["top_p"] = serde_json::json!(top_p);
                }
                if let Some(user) = &request.user {
                    body["user"] = serde_json::json!(user);
                }
                if let Some(metadata) = &request.metadata {
                    body["metadata"] = metadata.clone();
                }

                return Ok(("/v1/chat/completions", body));
            }

            let mut messages = Vec::new();

            for system in &request.system {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": system,
                }));
            }

            for message in &request.messages {
                let role = match message.role {
                    CanonicalRole::System => "system",
                    CanonicalRole::User => "user",
                    CanonicalRole::Assistant => "assistant",
                };
                let content = message
                    .content
                    .iter()
                    .map(|part| match part {
                        CanonicalContentPart::Text { text } => text.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                messages.push(serde_json::json!({
                    "role": role,
                    "content": content,
                }));
            }

            let mut body = serde_json::json!({
                "model": upstream_model,
                "messages": messages,
                "stream": request.stream,
            });

            if let Some(max_tokens) = request.max_tokens {
                body["max_tokens"] = serde_json::json!(max_tokens);
            }
            if !request.stop_sequences.is_empty() {
                body["stop"] = serde_json::json!(request.stop_sequences);
            }
            if let Some(temperature) = request.temperature {
                body["temperature"] = serde_json::json!(temperature);
            }
            if let Some(top_p) = request.top_p {
                body["top_p"] = serde_json::json!(top_p);
            }
            if let Some(user) = &request.user {
                body["user"] = serde_json::json!(user);
            }
            if let Some(metadata) = &request.metadata {
                body["metadata"] = metadata.clone();
            }

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
    use super::{build_upstream_request, extract_usage};
    use aisix_types::request::{
        CanonicalChatRequest, CanonicalContentPart, CanonicalMessage, CanonicalRequest,
        CanonicalRole, ProtocolFamily,
    };
    use serde_json::json;

    #[test]
    fn builds_openai_request_from_anthropic_canonical_chat() {
        let request = CanonicalRequest::Chat(CanonicalChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            system: vec!["be concise".to_string()],
            messages: vec![CanonicalMessage {
                role: CanonicalRole::User,
                content: vec![CanonicalContentPart::Text {
                    text: "hello".to_string(),
                }],
            }],
            raw_messages: None,
            stream: true,
            max_tokens: Some(256),
            stop_sequences: vec!["STOP".to_string()],
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(50),
            metadata: Some(json!({"trace_id": "abc"})),
            user: Some("user-1".to_string()),
            protocol: ProtocolFamily::Anthropic,
        });

        let (path, body) = build_upstream_request(&request, "deepseek-chat").unwrap();

        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["stop"], json!(["STOP"]));
        assert_eq!(body["temperature"].as_f64(), Some(0.20000000298023224));
        assert_eq!(body["top_p"].as_f64(), Some(0.8999999761581421));
        assert_eq!(body["user"], "user-1");
        assert_eq!(
            body["messages"],
            json!([
                {"role": "system", "content": "be concise"},
                {"role": "user", "content": "hello"}
            ])
        );
    }

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

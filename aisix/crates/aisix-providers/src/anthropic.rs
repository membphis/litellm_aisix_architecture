use aisix_config::etcd_model::ProviderConfig;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::CanonicalRequest,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{
    anthropic_sse::normalize_anthropic_chat_sse,
    codec::{JsonOutput, ProviderCodec, StreamOutput},
    openai_sse::OpenAiSseEvent,
};

#[derive(Debug, Clone, Default)]
pub struct AnthropicCodec {
    client: reqwest::Client,
}

#[async_trait]
impl ProviderCodec for AnthropicCodec {
    fn provider_id(&self) -> &'static str {
        "anthropic"
    }

    async fn execute_json(
        &self,
        _provider: &ProviderConfig,
        _upstream_model: &str,
        _request: &CanonicalRequest,
    ) -> Result<JsonOutput, GatewayError> {
        Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "anthropic does not support non-stream chat yet".to_string(),
        })
    }

    async fn execute_stream(
        &self,
        provider: &ProviderConfig,
        upstream_model: &str,
        request: &CanonicalRequest,
    ) -> Result<StreamOutput, GatewayError> {
        let secret = resolve_secret(&provider.auth.secret_ref)?;
        let body = build_upstream_request(request, upstream_model)?;
        let url = format!("{}/v1/messages", provider.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .header("x-api-key", secret)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
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

        let normalized = normalize_anthropic_chat_sse(&body)?;
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

fn build_upstream_request(
    request: &CanonicalRequest,
    upstream_model: &str,
) -> Result<Value, GatewayError> {
    match request {
        CanonicalRequest::Chat(request) => {
            let mut system_parts = Vec::new();
            let mut messages = Vec::new();

            for message in &request.messages {
                let Some(object) = message.as_object() else {
                    return Err(unsupported_message_shape(message));
                };
                if object.len() != 2 || !object.contains_key("role") || !object.contains_key("content") {
                    return Err(unsupported_message_shape(message));
                }

                let Some(role) = message.get("role").and_then(Value::as_str) else {
                    return Err(unsupported_message_shape(message));
                };
                let Some(content) = message.get("content") else {
                    return Err(unsupported_message_shape(message));
                };

                if role == "system" {
                    let Some(content) = content.as_str() else {
                        return Err(unsupported_message_shape(message));
                    };
                    system_parts.push(content.to_string());
                    continue;
                }

                if role != "user" && role != "assistant" {
                    return Err(unsupported_message_shape(message));
                }

                if !content.is_string() {
                    return Err(unsupported_message_shape(message));
                }

                messages.push(json!({
                    "role": role,
                    "content": content,
                }));
            }

            let mut body = json!({
                "model": upstream_model,
                "stream": true,
                "max_tokens": 1024,
                "messages": messages,
            });

            if !system_parts.is_empty() {
                body["system"] = Value::String(system_parts.join("\n\n"));
            }

            Ok(body)
        }
        CanonicalRequest::Embeddings(_) => Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "anthropic does not support embeddings".to_string(),
        }),
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

fn unsupported_message_shape(message: &Value) -> GatewayError {
    GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: format!(
            "unsupported Anthropic message shape for Phase 1: {}",
            serde_json::to_string(message).unwrap_or_else(|_| "<unserializable message>".to_string())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::build_upstream_request;
    use aisix_types::{
        error::ErrorKind,
        request::{CanonicalRequest, ChatRequest},
    };
    use serde_json::json;

    #[test]
    fn rejects_unsupported_tool_role_messages() {
        let request = CanonicalRequest::Chat(ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![json!({
                "role": "tool",
                "content": "tool output"
            })],
            stream: true,
        });

        let error = build_upstream_request(&request, "claude-3-5-haiku-latest")
            .expect_err("tool role should be rejected");

        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(error.message.contains("unsupported Anthropic message shape"));
    }

    #[test]
    fn anthropic_sse_rejects_non_string_message_content() {
        let request = CanonicalRequest::Chat(ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![json!({
                "role": "user",
                "content": {"text": "hello"}
            })],
            stream: true,
        });

        let error = build_upstream_request(&request, "claude-3-5-haiku-latest")
            .expect_err("non-string content should be rejected");

        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(error.message.contains("unsupported Anthropic message shape"));
    }

    #[test]
    fn rejects_supported_role_messages_with_extra_fields() {
        let request = CanonicalRequest::Chat(ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![json!({
                "role": "assistant",
                "content": "hello",
                "tool_calls": []
            })],
            stream: true,
        });

        let error = build_upstream_request(&request, "claude-3-5-haiku-latest")
            .expect_err("extra unsupported fields should be rejected");

        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(error.message.contains("unsupported Anthropic message shape"));
    }
}

use aisix_config::etcd_model::ProviderConfig;
use aisix_types::{
    error::{ErrorKind, GatewayError},
    request::{CanonicalContentPart, CanonicalRequest, CanonicalRole},
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
            let mut system_parts = request.system.clone();
            let mut messages = Vec::new();

            for message in &request.messages {
                let role = match message.role {
                    CanonicalRole::System => {
                        system_parts.push(join_text_parts(&message.content));
                        continue;
                    }
                    CanonicalRole::User => "user",
                    CanonicalRole::Assistant => "assistant",
                };
                let content = join_text_parts(&message.content);

                messages.push(json!({
                    "role": role,
                    "content": content,
                }));
            }

            let mut body = json!({
                "model": upstream_model,
                "stream": true,
                "max_tokens": request.max_tokens.unwrap_or(1024),
                "messages": messages,
            });

            if !system_parts.is_empty() {
                body["system"] = Value::String(system_parts.join("\n\n"));
            }
            if !request.stop_sequences.is_empty() {
                body["stop_sequences"] = serde_json::json!(request.stop_sequences);
            }
            if let Some(temperature) = request.temperature {
                body["temperature"] = serde_json::json!(temperature);
            }
            if let Some(top_p) = request.top_p {
                body["top_p"] = serde_json::json!(top_p);
            }
            if let Some(top_k) = request.top_k {
                body["top_k"] = serde_json::json!(top_k);
            }
            if let Some(metadata) = &request.metadata {
                body["metadata"] = metadata.clone();
            }

            Ok(body)
        }
        CanonicalRequest::Embeddings(_) => Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "anthropic does not support embeddings".to_string(),
        }),
    }
}

fn join_text_parts(parts: &[CanonicalContentPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            CanonicalContentPart::Text { text } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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

fn strip_unsafe_body_headers(
    mut headers: reqwest::header::HeaderMap,
) -> reqwest::header::HeaderMap {
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

#[cfg(test)]
mod tests {
    use super::build_upstream_request;
    use aisix_types::request::{
        CanonicalChatRequest, CanonicalContentPart, CanonicalMessage, CanonicalRequest,
        CanonicalRole, ProtocolFamily,
    };

    fn canonical_request(role: CanonicalRole, text: &str) -> CanonicalRequest {
        CanonicalRequest::Chat(CanonicalChatRequest {
            model: "gpt-4o-mini".to_string(),
            system: vec![],
            messages: vec![CanonicalMessage {
                role,
                content: vec![CanonicalContentPart::Text {
                    text: text.to_string(),
                }],
            }],
            raw_messages: None,
            stream: true,
            max_tokens: None,
            stop_sequences: vec![],
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            user: None,
            protocol: ProtocolFamily::OpenAi,
        })
    }

    #[test]
    fn anthropic_upstream_request_maps_system_and_message_content() {
        let request = CanonicalRequest::Chat(CanonicalChatRequest {
            model: "gpt-4o-mini".to_string(),
            system: vec!["be concise".to_string()],
            messages: vec![CanonicalMessage {
                role: CanonicalRole::User,
                content: vec![CanonicalContentPart::Text {
                    text: "hello".to_string(),
                }],
            }],
            raw_messages: None,
            stream: true,
            max_tokens: Some(2048),
            stop_sequences: vec![],
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            user: None,
            protocol: ProtocolFamily::OpenAi,
        });

        let body = build_upstream_request(&request, "claude-3-5-haiku-latest").unwrap();

        assert_eq!(body["model"], "claude-3-5-haiku-latest");
        assert_eq!(body["system"], "be concise");
        assert_eq!(body["max_tokens"], 2048);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn anthropic_upstream_request_keeps_assistant_messages() {
        let request = canonical_request(CanonicalRole::Assistant, "hello");

        let body = build_upstream_request(&request, "claude-3-5-haiku-latest").unwrap();

        assert_eq!(body["messages"][0]["role"], "assistant");
        assert_eq!(body["messages"][0]["content"], "hello");
    }
}

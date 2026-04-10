use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorKind, GatewayError};
pub use crate::usage::TransportMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolFamily {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalContentPart {
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub role: CanonicalRole,
    pub content: Vec<CanonicalContentPart>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalChatRequest {
    pub model: String,
    pub system: Vec<String>,
    pub messages: Vec<CanonicalMessage>,
    pub raw_messages: Option<Vec<Value>>,
    pub stream: bool,
    pub max_tokens: Option<u32>,
    pub stop_sequences: Vec<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub metadata: Option<Value>,
    pub user: Option<String>,
    pub protocol: ProtocolFamily,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CanonicalRequest {
    Chat(CanonicalChatRequest),
    Embeddings(EmbeddingsRequest),
}

impl CanonicalRequest {
    pub fn model_name(&self) -> &str {
        match self {
            Self::Chat(request) => &request.model,
            Self::Embeddings(request) => &request.model,
        }
    }

    pub fn transport_mode(&self) -> TransportMode {
        match self {
            Self::Chat(request) if request.stream => TransportMode::SseStream,
            Self::Chat(_) | Self::Embeddings(_) => TransportMode::Json,
        }
    }
}

impl ChatRequest {
    pub fn into_canonical(self) -> Result<CanonicalChatRequest, GatewayError> {
        let raw_messages = self.messages.clone();
        let mut system = Vec::new();
        let mut messages = Vec::new();

        for message in self.messages {
            let Some(object) = message.as_object() else {
                return Err(invalid_openai_message(&message));
            };
            let Some(role) = object.get("role").and_then(Value::as_str) else {
                return Err(invalid_openai_message(&message));
            };
            let Some(content) = object.get("content") else {
                return Err(invalid_openai_message(&message));
            };

            let parts = normalize_openai_content(content)?;
            match role {
                "system" => system.push(join_text_parts(&parts)),
                "user" => messages.push(CanonicalMessage {
                    role: CanonicalRole::User,
                    content: parts,
                }),
                "assistant" => messages.push(CanonicalMessage {
                    role: CanonicalRole::Assistant,
                    content: parts,
                }),
                _ => {
                    return Err(GatewayError {
                        kind: ErrorKind::InvalidRequest,
                        message: format!("unsupported OpenAI chat role: {role}"),
                    });
                }
            }
        }

        Ok(CanonicalChatRequest {
            model: self.model,
            system,
            messages,
            raw_messages: Some(raw_messages),
            stream: self.stream,
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
}

fn normalize_openai_content(content: &Value) -> Result<Vec<CanonicalContentPart>, GatewayError> {
    if let Some(text) = content.as_str() {
        return Ok(vec![CanonicalContentPart::Text {
            text: text.to_string(),
        }]);
    }

    let Some(parts) = content.as_array() else {
        return Err(invalid_openai_message(content));
    };

    let mut normalized = Vec::with_capacity(parts.len());
    for part in parts {
        let Some(object) = part.as_object() else {
            return Err(invalid_openai_message(content));
        };
        if object.get("type").and_then(Value::as_str) != Some("text") {
            return Err(invalid_openai_message(content));
        }
        let Some(text) = object.get("text").and_then(Value::as_str) else {
            return Err(invalid_openai_message(content));
        };
        normalized.push(CanonicalContentPart::Text {
            text: text.to_string(),
        });
    }

    Ok(normalized)
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

fn invalid_openai_message(value: &Value) -> GatewayError {
    GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: format!(
            "unsupported OpenAI chat message shape: {}",
            serde_json::to_string(value).unwrap_or_else(|_| "<unserializable message>".to_string())
        ),
    }
}

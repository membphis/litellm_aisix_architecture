use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::usage::TransportMode;

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
    Chat(ChatRequest),
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

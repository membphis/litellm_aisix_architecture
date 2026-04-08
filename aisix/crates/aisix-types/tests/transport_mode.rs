use aisix_types::request::{CanonicalRequest, ChatRequest, EmbeddingsRequest, TransportMode};

#[test]
fn transport_mode_matches_request_shape() {
    let chat_stream = CanonicalRequest::Chat(ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![],
        stream: true,
    });
    assert_eq!(chat_stream.transport_mode(), TransportMode::SseStream);

    let chat_non_stream = CanonicalRequest::Chat(ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![],
        stream: false,
    });
    assert_eq!(chat_non_stream.transport_mode(), TransportMode::Json);

    let embeddings = CanonicalRequest::Embeddings(EmbeddingsRequest {
        model: "text-embedding-3-small".to_string(),
        input: serde_json::json!("hello"),
    });
    assert_eq!(embeddings.transport_mode(), TransportMode::Json);
}

#[test]
fn chat_request_without_stream_defaults_to_json_transport() {
    let chat_request: ChatRequest = serde_json::from_value(serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": []
    }))
    .expect("chat request should deserialize without stream field");

    let request = CanonicalRequest::Chat(chat_request);
    assert_eq!(request.transport_mode(), TransportMode::Json);
}

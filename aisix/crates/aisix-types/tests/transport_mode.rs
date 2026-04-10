use aisix_types::request::{
    CanonicalChatRequest, CanonicalRequest, ChatRequest, EmbeddingsRequest, ProtocolFamily,
    TransportMode,
};

fn canonical_chat_request(stream: bool) -> CanonicalChatRequest {
    CanonicalChatRequest {
        model: "gpt-4o-mini".to_string(),
        system: vec![],
        messages: vec![],
        raw_messages: None,
        stream,
        max_tokens: None,
        stop_sequences: vec![],
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        user: None,
        protocol: ProtocolFamily::OpenAi,
    }
}

#[test]
fn transport_mode_matches_request_shape() {
    let chat_stream = CanonicalRequest::Chat(canonical_chat_request(true));
    assert_eq!(chat_stream.transport_mode(), TransportMode::SseStream);

    let chat_non_stream = CanonicalRequest::Chat(canonical_chat_request(false));
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

    let request = CanonicalRequest::Chat(CanonicalChatRequest {
        model: chat_request.model,
        system: vec![],
        messages: vec![],
        raw_messages: Some(chat_request.messages),
        stream: chat_request.stream,
        max_tokens: None,
        stop_sequences: vec![],
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        user: None,
        protocol: ProtocolFamily::OpenAi,
    });
    assert_eq!(request.transport_mode(), TransportMode::Json);
}

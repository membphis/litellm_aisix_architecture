use aisix_types::{
    anthropic::AnthropicMessagesRequest,
    request::{
        CanonicalChatRequest, CanonicalContentPart, CanonicalMessage, CanonicalRequest,
        CanonicalRole, ProtocolFamily,
    },
    usage::TransportMode,
};
use serde_json::json;

#[test]
fn canonical_chat_transport_mode_tracks_stream_flag() {
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

    assert_eq!(request.model_name(), "claude-3-5-sonnet");
    assert_eq!(request.transport_mode(), TransportMode::SseStream);
}

#[test]
fn anthropic_request_accepts_text_content_shapes() {
    let request: AnthropicMessagesRequest = serde_json::from_value(json!({
        "model": "claude-3-5-sonnet",
        "system": [{"type": "text", "text": "be concise"}],
        "messages": [
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}
        ],
        "max_tokens": 256,
        "stop_sequences": ["STOP"],
        "metadata": {"trace_id": "abc"},
        "stream": false
    }))
    .expect("anthropic request should deserialize");

    assert_eq!(request.model, "claude-3-5-sonnet");
    assert_eq!(request.stop_sequences, vec!["STOP"]);
    assert!(!request.stream);
}

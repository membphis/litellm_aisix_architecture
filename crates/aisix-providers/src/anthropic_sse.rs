use aisix_types::{
    error::{ErrorKind, GatewayError},
    usage::Usage,
};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::openai_sse::{NormalizedOpenAiSse, OpenAiSseEvent};

fn merge_usage(current: Option<&Usage>, usage_json: &Value) -> Usage {
    Usage {
        input_tokens: usage_json
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| current.map(|usage| usage.input_tokens).unwrap_or(0)),
        output_tokens: usage_json
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| current.map(|usage| usage.output_tokens).unwrap_or(0)),
        cache_read: usage_json
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| current.map(|usage| usage.cache_read).unwrap_or(0)),
        cache_write: usage_json
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| current.map(|usage| usage.cache_write).unwrap_or(0)),
    }
}

pub fn normalize_anthropic_chat_sse(body: &[u8]) -> Result<NormalizedOpenAiSse, GatewayError> {
    let text = std::str::from_utf8(body).map_err(|error| GatewayError {
        kind: ErrorKind::Upstream,
        message: format!("upstream SSE body was not valid UTF-8: {error}"),
    })?;
    let text = text.replace("\r\n", "\n");

    let mut events = Vec::new();
    let mut usage: Option<Usage> = None;
    let mut message_id = None;
    let mut finish_reason = None;
    let mut content_deltas = Vec::new();

    for frame in text.split("\n\n") {
        let mut event_name = None;
        let mut data_lines = Vec::new();

        for line in frame.lines() {
            if let Some(event) = line.strip_prefix("event:") {
                event_name = Some(event.trim());
            }
            if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let payload = data_lines.join("\n");
        let json: Value = serde_json::from_str(&payload).map_err(|error| GatewayError {
            kind: ErrorKind::Upstream,
            message: format!("failed to parse upstream SSE frame: {error}"),
        })?;

        match event_name.or_else(|| json.get("type").and_then(Value::as_str)) {
            Some("message_start") => {
                if let Some(message) = json.get("message") {
                    message_id = message
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(message_id);

                    if let Some(message_usage) = message.get("usage") {
                        usage = Some(merge_usage(usage.as_ref(), message_usage));
                    }
                }
            }
            Some("content_block_delta") => {
                let Some(text) = json
                    .get("delta")
                    .and_then(|delta| delta.get("text"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                content_deltas.push(text.to_string());
            }
            Some("message_delta") => {
                if let Some(delta_usage) = json.get("usage") {
                    usage = Some(merge_usage(usage.as_ref(), delta_usage));
                }

                if let Some(stop_reason) = json
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    if stop_reason == "tool_use" {
                        return Err(GatewayError {
                            kind: ErrorKind::InvalidRequest,
                            message: "Anthropic tool_use streaming is not supported in Phase 1 normalization"
                                .to_string(),
                        });
                    }

                    finish_reason = Some(map_finish_reason(stop_reason));
                }
            }
            Some("message_stop") => {
                if let Some(message) = json.get("message") {
                    message_id = message
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(message_id);

                    if let Some(message_usage) = message.get("usage") {
                        usage = Some(merge_usage(usage.as_ref(), message_usage));
                    }
                }

                let chunk_id = message_id
                    .clone()
                    .unwrap_or_else(|| "anthropic".to_string());
                for text in &content_deltas {
                    let chunk = json!({
                        "id": chunk_id,
                        "object": "chat.completion.chunk",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "role": "assistant",
                                "content": text,
                            },
                            "finish_reason": Value::Null,
                        }]
                    });
                    events.push(OpenAiSseEvent::Data(serialize_event(&chunk)?));
                }

                let final_usage = usage.as_ref().map(|usage| {
                    json!({
                        "prompt_tokens": usage.input_tokens,
                        "completion_tokens": usage.output_tokens,
                        "total_tokens": usage.input_tokens + usage.output_tokens,
                    })
                });
                let chunk = json!({
                    "id": chunk_id,
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason.unwrap_or("stop"),
                    }],
                    "usage": final_usage,
                });
                events.push(OpenAiSseEvent::Data(serialize_event(&chunk)?));
                events.push(OpenAiSseEvent::Done);
            }
            _ => {}
        }
    }

    Ok(NormalizedOpenAiSse { events, usage })
}

fn serialize_event(value: &Value) -> Result<Bytes, GatewayError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to serialize normalized SSE frame: {error}"),
        })
}

fn map_finish_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        _ => "stop",
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_anthropic_chat_sse;
    use crate::openai_sse::OpenAiSseEvent;
    use aisix_types::error::ErrorKind;

    #[test]
    fn normalizes_anthropic_chat_stream_to_openai_chunks() {
        let stream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":7}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3,\"cache_read_input_tokens\":5}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );

        let normalized = normalize_anthropic_chat_sse(stream.as_bytes()).unwrap();

        assert_eq!(normalized.events.len(), 3);
        assert!(matches!(normalized.events[0], OpenAiSseEvent::Data(_)));
        assert!(matches!(normalized.events[1], OpenAiSseEvent::Data(_)));
        assert_eq!(normalized.events[2], OpenAiSseEvent::Done);
        if let OpenAiSseEvent::Data(first) = &normalized.events[0] {
            let first = String::from_utf8(first.to_vec()).unwrap();
            assert!(first.contains("\"id\":\"msg_test\""));
        }
        let usage = normalized.usage.expect("usage should be extracted");
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.cache_read, 5);
        assert_eq!(usage.cache_write, 7);
        if let OpenAiSseEvent::Data(last_chunk) = &normalized.events[1] {
            let last_chunk: serde_json::Value = serde_json::from_slice(last_chunk).unwrap();
            assert_eq!(last_chunk["choices"][0]["finish_reason"], "stop");
        }
    }

    #[test]
    fn maps_max_tokens_stop_reason_to_length() {
        let stream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":2}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );

        let normalized = normalize_anthropic_chat_sse(stream.as_bytes()).unwrap();

        if let OpenAiSseEvent::Data(last_chunk) = &normalized.events[1] {
            let last_chunk: serde_json::Value = serde_json::from_slice(last_chunk).unwrap();
            assert_eq!(last_chunk["choices"][0]["finish_reason"], "length");
        }
    }

    #[test]
    fn rejects_tool_use_stop_reason_without_tool_call_payloads() {
        let stream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n"
        );

        let error = normalize_anthropic_chat_sse(stream.as_bytes())
            .expect_err("tool_use should be rejected until tool-call deltas are implemented");

        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(error.message.contains("tool_use"));
    }
}

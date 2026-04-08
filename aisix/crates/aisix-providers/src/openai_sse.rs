use aisix_types::{
    error::{ErrorKind, GatewayError},
    usage::Usage,
};
use bytes::Bytes;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum OpenAiSseEvent {
    Data(Bytes),
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedOpenAiSse {
    pub events: Vec<OpenAiSseEvent>,
    pub usage: Option<Usage>,
}

pub fn normalize_openai_chat_sse(body: &[u8]) -> Result<NormalizedOpenAiSse, GatewayError> {
    let text = std::str::from_utf8(body).map_err(|error| GatewayError {
        kind: ErrorKind::Upstream,
        message: format!("upstream SSE body was not valid UTF-8: {error}"),
    })?;
    let text = text.replace("\r\n", "\n");

    let mut events = Vec::new();
    let mut usage = None;

    for frame in text.split("\n\n") {
        let data_lines = frame
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>();

        if data_lines.is_empty() {
            continue;
        }

        let payload = data_lines.join("\n");
        if payload == "[DONE]" {
            events.push(OpenAiSseEvent::Done);
            continue;
        }

        let json: Value = serde_json::from_str(&payload).map_err(|error| GatewayError {
            kind: ErrorKind::Upstream,
            message: format!("failed to parse upstream SSE frame: {error}"),
        })?;
        usage = usage.or_else(|| extract_usage(&json));
        let normalized = serde_json::to_vec(&json).map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to serialize normalized SSE frame: {error}"),
        })?;
        events.push(OpenAiSseEvent::Data(Bytes::from(normalized)));
    }

    Ok(NormalizedOpenAiSse { events, usage })
}

fn extract_usage(json: &Value) -> Option<Usage> {
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
    use super::{normalize_openai_chat_sse, NormalizedOpenAiSse, OpenAiSseEvent};
    use bytes::Bytes;

    #[test]
    fn normalizes_openai_chat_stream_and_preserves_done() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: [DONE]\n\n"
        );

        let normalized = normalize_openai_chat_sse(stream.as_bytes()).unwrap();

        assert_eq!(
            normalized,
            NormalizedOpenAiSse {
                events: vec![
                    OpenAiSseEvent::Data(Bytes::from_static(
                        b"{\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}"
                    )),
                    OpenAiSseEvent::Done,
                ],
                usage: None,
            }
        );
    }

    #[test]
    fn normalizes_crlf_framed_openai_chat_stream() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\n",
            "data: [DONE]\r\n\r\n"
        );

        let normalized = normalize_openai_chat_sse(stream.as_bytes()).unwrap();

        assert_eq!(normalized.events.len(), 2);
        assert!(matches!(normalized.events[0], OpenAiSseEvent::Data(_)));
        assert_eq!(normalized.events[1], OpenAiSseEvent::Done);
    }
}

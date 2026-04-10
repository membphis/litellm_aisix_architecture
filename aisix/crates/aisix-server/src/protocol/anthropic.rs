use aisix_providers::openai_sse::{normalize_openai_chat_sse, OpenAiSseEvent};
use aisix_types::{
    anthropic::{AnthropicInputMessage, AnthropicMessagesRequest},
    error::{AnthropicErrorBody, AnthropicErrorResponse, ErrorKind, GatewayError},
    request::{
        CanonicalChatRequest, CanonicalContentPart, CanonicalMessage, CanonicalRole, ProtocolFamily,
    },
    usage::Usage,
};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use bytes::Bytes;
use serde_json::{json, Value};

pub fn require_anthropic_version(headers: &HeaderMap) -> Result<String, GatewayError> {
    headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: "missing required anthropic-version header".to_string(),
        })
}

pub fn into_canonical_request(
    request: AnthropicMessagesRequest,
) -> Result<CanonicalChatRequest, GatewayError> {
    reject_unsupported_top_level_fields(&request)?;

    Ok(CanonicalChatRequest {
        model: request.model,
        system: normalize_system(request.system)?,
        messages: normalize_messages(request.messages)?,
        raw_messages: None,
        stream: request.stream,
        max_tokens: request.max_tokens,
        stop_sequences: request.stop_sequences,
        temperature: request.temperature,
        top_p: request.top_p,
        top_k: request.top_k,
        metadata: request.metadata,
        user: request.user,
        protocol: ProtocolFamily::Anthropic,
    })
}

pub fn build_anthropic_json_response(
    status: StatusCode,
    body: &[u8],
    usage: Option<Usage>,
    model: &str,
) -> Result<Response<Body>, GatewayError> {
    if !status.is_success() {
        return build_anthropic_error_response(status, body);
    }

    let upstream: Value = serde_json::from_slice(body).map_err(|error| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("failed to parse openai json response: {error}"),
    })?;

    let text = upstream["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();

    let anthropic = json!({
        "id": upstream["id"],
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": text}],
        "stop_reason": map_finish_reason(upstream["choices"][0]["finish_reason"].as_str()),
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.as_ref().map(|value| value.input_tokens).unwrap_or(0),
            "output_tokens": usage.as_ref().map(|value| value.output_tokens).unwrap_or(0)
        }
    });

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&anthropic).map_err(
            |error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize anthropic json response: {error}"),
            },
        )?))
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build anthropic json response: {error}"),
        })
}

pub fn build_anthropic_error_response(
    status: StatusCode,
    body: &[u8],
) -> Result<Response<Body>, GatewayError> {
    let payload: Value = serde_json::from_slice(body).unwrap_or_else(|_| json!({}));
    let error_type = payload
        .get("error")
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| anthropic_error_type(status));
    let message = payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request failed"));

    render_error(status, error_type, message)
}

pub fn render_gateway_error(error: GatewayError) -> Response<Body> {
    render_error(
        error.status_code(),
        anthropic_error_type(error.status_code()),
        &error.message,
    )
    .unwrap_or_else(|build_error| build_error.into_response())
}

pub fn map_finish_reason(reason: Option<&str>) -> Value {
    match reason {
        Some("stop") => Value::String("end_turn".to_string()),
        Some("length") => Value::String("max_tokens".to_string()),
        Some("content_filter") => Value::String("stop_sequence".to_string()),
        Some(other) => Value::String(other.to_string()),
        None => Value::Null,
    }
}

pub fn build_anthropic_stream_proxy_response(
    status: StatusCode,
    body: Bytes,
    headers: HeaderMap,
    provider_id: &str,
    usage: Option<Usage>,
    model: &str,
) -> Result<Response<Body>, GatewayError> {
    if !status.is_success() {
        return build_anthropic_error_response(status, &body);
    }

    let normalized = normalize_openai_chat_sse(&body)?;
    let parsed_events = normalized
        .events
        .iter()
        .filter_map(|event| match event {
            OpenAiSseEvent::Data(data) => Some(data),
            OpenAiSseEvent::Done => None,
        })
        .map(|data| {
            serde_json::from_slice::<Value>(data).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to parse normalized openai sse frame: {error}"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut rendered = Vec::new();
    let message_id = parsed_events
        .iter()
        .find_map(|json| json["id"].as_str())
        .unwrap_or("msg_aisix");

    rendered.extend_from_slice(
        format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":{},\"type\":\"message\",\"role\":\"assistant\",\"model\":\"{}\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":{},\"output_tokens\":0}}}}}}\n\n",
            serde_json::to_string(message_id).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize anthropic message id: {error}"),
            })?,
            model,
            usage.as_ref().map(|value| value.input_tokens).unwrap_or(0)
        )
        .as_bytes(),
    );
    rendered.extend_from_slice(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n");

    let mut finish_reason = None;
    for json in parsed_events {
        if let Some(text) = json["choices"][0]["delta"]["content"].as_str() {
            rendered.extend_from_slice(
                format!(
                    "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{}}}}}\n\n",
                    serde_json::to_string(text).map_err(|error| GatewayError {
                        kind: ErrorKind::Internal,
                        message: format!("failed to serialize anthropic text delta: {error}"),
                    })?
                )
                .as_bytes(),
            );
        }

        finish_reason = finish_reason.or_else(|| {
            json["choices"][0]["finish_reason"]
                .as_str()
                .map(str::to_string)
        });
    }

    rendered.extend_from_slice(
        b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    );
    rendered.extend_from_slice(
        format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":{},\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":{}}}}}\n\n",
            serde_json::to_string(&map_finish_reason(finish_reason.as_deref())).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize anthropic stop reason: {error}"),
            })?,
            usage.as_ref().map(|value| value.output_tokens).unwrap_or(0)
        )
        .as_bytes(),
    );

    rendered.extend_from_slice(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");

    let mut response = crate::stream_proxy::build_stream_response(
        status,
        Body::from(rendered),
        headers,
        provider_id,
        usage,
    )?;
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/event-stream"),
    );
    Ok(response)
}

fn reject_unsupported_top_level_fields(
    request: &AnthropicMessagesRequest,
) -> Result<(), GatewayError> {
    for (field, present) in [
        ("tools", request.tools.is_some()),
        ("tool_choice", request.tool_choice.is_some()),
        ("thinking", request.thinking.is_some()),
        ("container", request.container.is_some()),
    ] {
        if present {
            return Err(unsupported_field_error(field));
        }
    }

    if let Some(field) = request.extra_fields.keys().next() {
        return Err(unsupported_field_error(field));
    }

    Ok(())
}

fn normalize_system(system: Option<Value>) -> Result<Vec<String>, GatewayError> {
    match system {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(text)) => Ok(vec![text]),
        Some(Value::Array(parts)) => parts.into_iter().map(normalize_text_value).collect(),
        Some(other) => Err(invalid_request(format!(
            "unsupported Anthropic system shape: {}",
            serialize_value(&other)
        ))),
    }
}

fn normalize_messages(
    messages: Vec<AnthropicInputMessage>,
) -> Result<Vec<CanonicalMessage>, GatewayError> {
    messages
        .into_iter()
        .map(|message| {
            let role = match message.role.as_str() {
                "user" => CanonicalRole::User,
                "assistant" => CanonicalRole::Assistant,
                other => {
                    return Err(invalid_request(format!(
                        "unsupported Anthropic message role: {other}"
                    )))
                }
            };

            Ok(CanonicalMessage {
                role,
                content: normalize_content(message.content)?,
            })
        })
        .collect()
}

fn normalize_content(content: Value) -> Result<Vec<CanonicalContentPart>, GatewayError> {
    match content {
        Value::String(text) => Ok(vec![CanonicalContentPart::Text { text }]),
        Value::Array(parts) => parts
            .into_iter()
            .map(|part| {
                let Some(object) = part.as_object() else {
                    return Err(invalid_request(format!(
                        "unsupported Anthropic content block: {}",
                        serialize_value(&part)
                    )));
                };
                let Some(kind) = object.get("type").and_then(Value::as_str) else {
                    return Err(invalid_request(format!(
                        "unsupported Anthropic content block: {}",
                        serialize_value(&part)
                    )));
                };
                if kind != "text" {
                    return Err(unsupported_field_error(kind));
                }
                let Some(text) = object.get("text").and_then(Value::as_str) else {
                    return Err(invalid_request(format!(
                        "unsupported Anthropic content block: {}",
                        serialize_value(&part)
                    )));
                };

                Ok(CanonicalContentPart::Text {
                    text: text.to_string(),
                })
            })
            .collect(),
        other => Err(invalid_request(format!(
            "unsupported Anthropic content shape: {}",
            serialize_value(&other)
        ))),
    }
}

fn normalize_text_value(value: Value) -> Result<String, GatewayError> {
    match value {
        Value::String(text) => Ok(text),
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("text") => {
            object
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| invalid_request("unsupported Anthropic text block".to_string()))
        }
        other => Err(invalid_request(format!(
            "unsupported Anthropic text block: {}",
            serialize_value(&other)
        ))),
    }
}

fn render_error(
    status: StatusCode,
    error_type: &str,
    message: &str,
) -> Result<Response<Body>, GatewayError> {
    let envelope = json!({
        "type": "error",
        "error": AnthropicErrorResponse {
            error: AnthropicErrorBody {
                error_type: error_type.to_string(),
                message: message.to_string(),
            }
        }
        .error
    });

    let mut response = Response::builder()
        .status(status)
        .body(Body::from(serde_json::to_vec(&envelope).map_err(
            |error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to serialize anthropic error response: {error}"),
            },
        )?))
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build anthropic error response: {error}"),
        })?;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    Ok(response)
}

fn anthropic_error_type(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND => "invalid_request_error",
        StatusCode::SERVICE_UNAVAILABLE => "overloaded_error",
        _ => "api_error",
    }
}

fn unsupported_field_error(field: &str) -> GatewayError {
    invalid_request(format!(
        "unsupported Anthropic field in current implementation: {field}"
    ))
}

fn invalid_request(message: String) -> GatewayError {
    GatewayError {
        kind: ErrorKind::InvalidRequest,
        message,
    }
}

fn serialize_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable value>".to_string())
}

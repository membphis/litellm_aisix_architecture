use aisix_types::error::{ErrorKind, GatewayError};
use axum::{
    body::Body,
    http::{header, HeaderName, HeaderValue, Response},
};

pub(super) fn build_json_response(
    status: http::StatusCode,
    body: impl Into<Body>,
    mut headers: http::HeaderMap,
    cache_hit: Option<&str>,
    provider_id: Option<&str>,
    usage: Option<aisix_types::usage::Usage>,
) -> Result<Response<Body>, GatewayError> {
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::CONNECTION);

    if cache_hit.is_some() && !headers.contains_key(http::header::CONTENT_TYPE) {
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }

    let mut response = Response::builder()
        .status(status)
        .body(body.into())
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build response: {error}"),
        })?;

    response.headers_mut().extend(headers);

    if let Some(cache_hit) = cache_hit {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-cache-hit"),
            HeaderValue::from_str(cache_hit).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set cache header: {error}"),
            })?,
        );
    }

    if let Some(provider_id) = provider_id {
        response.headers_mut().insert(
            HeaderName::from_static("x-aisix-provider"),
            HeaderValue::from_str(provider_id).map_err(|error| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("failed to set provider header: {error}"),
            })?,
        );
    }

    if let Some(usage) = usage {
        response.extensions_mut().insert(usage);
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{HeaderMap, HeaderValue, StatusCode},
    };

    use super::build_json_response;

    #[test]
    fn build_json_response_strips_stale_body_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("content-length", HeaderValue::from_static("999"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));

        let response = build_json_response(
            StatusCode::OK,
            Body::from("{\"ok\":true}"),
            headers,
            Some("false"),
            Some("openai"),
            None,
        )
        .unwrap();

        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert!(response.headers().get("transfer-encoding").is_none());
        assert_ne!(
            response
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok()),
            Some("999")
        );
    }

    #[test]
    fn build_json_response_preserves_status() {
        let response = build_json_response(
            StatusCode::ACCEPTED,
            Body::from("{}"),
            HeaderMap::new(),
            Some("false"),
            Some("openai"),
            None,
        )
        .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
}

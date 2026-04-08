use aisix_types::{
    error::{ErrorKind, GatewayError},
    usage::Usage,
};
use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderName, HeaderValue, Response},
};

pub fn build_stream_response(
    status: http::StatusCode,
    body: impl Into<Body>,
    headers: HeaderMap,
    provider_id: &str,
    usage: Option<Usage>,
) -> Result<Response<Body>, GatewayError> {
    let mut headers = sanitize_rebuilt_response_headers(headers);

    if !status.is_success() {
        return build_passthrough_response(status, body, headers, provider_id, usage);
    }

    headers.remove(header::CONTENT_TYPE);

    let mut response = Response::builder()
        .status(status)
        .body(body.into())
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build stream response: {error}"),
        })?;

    response.headers_mut().extend(headers);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-aisix-provider"),
        HeaderValue::from_str(provider_id).map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to set provider header: {error}"),
        })?,
    );

    if let Some(usage) = usage {
        response.extensions_mut().insert(usage);
    }

    Ok(response)
}

fn sanitize_rebuilt_response_headers(mut headers: HeaderMap) -> HeaderMap {
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::CONNECTION);
    headers
}

fn build_passthrough_response(
    status: http::StatusCode,
    body: impl Into<Body>,
    headers: HeaderMap,
    provider_id: &str,
    usage: Option<Usage>,
) -> Result<Response<Body>, GatewayError> {
    let mut response = Response::builder()
        .status(status)
        .body(body.into())
        .map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to build passthrough stream response: {error}"),
        })?;

    response.headers_mut().extend(headers);
    response.headers_mut().insert(
        HeaderName::from_static("x-aisix-provider"),
        HeaderValue::from_str(provider_id).map_err(|error| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("failed to set provider header: {error}"),
        })?,
    );

    if let Some(usage) = usage {
        response.extensions_mut().insert(usage);
    }

    Ok(response)
}

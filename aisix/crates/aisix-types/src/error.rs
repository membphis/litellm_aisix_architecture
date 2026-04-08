use axum::{
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Permission,
    NotFound,
    InvalidRequest,
    RateLimited,
    Timeout,
    Upstream,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayError {
    pub kind: ErrorKind,
    pub message: String,
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::Authentication => StatusCode::UNAUTHORIZED,
            ErrorKind::Permission => StatusCode::FORBIDDEN,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
            ErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Upstream => StatusCode::BAD_GATEWAY,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(OpenAiErrorResponse {
            error: OpenAiErrorBody {
                message: self.message,
                error_type: error_type(status).to_string(),
            },
        });

        (status, body).into_response()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiErrorResponse {
    pub error: OpenAiErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
}

fn error_type(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::NOT_FOUND => "invalid_request_error",
        StatusCode::BAD_REQUEST => "invalid_request_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        _ => "server_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_too_many_requests_to_rate_limit_error() {
        assert_eq!(
            error_type(StatusCode::TOO_MANY_REQUESTS),
            "rate_limit_error"
        );
    }
}

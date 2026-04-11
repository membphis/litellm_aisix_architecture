use aisix_types::error::{ErrorKind, GatewayError};
use axum::http::HeaderMap;

use crate::{admin::AdminState, app::ServerState};

pub fn require_admin(state: &ServerState, headers: &HeaderMap) -> Result<AdminState, GatewayError> {
    let admin = state.admin.clone().ok_or_else(|| GatewayError {
        kind: ErrorKind::Permission,
        message: "admin api is disabled".to_string(),
    })?;
    let supplied = headers
        .get("x-admin-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(invalid_admin_key)?;

    if !admin.is_authorized(supplied) {
        return Err(invalid_admin_key());
    }

    Ok(admin)
}

fn invalid_admin_key() -> GatewayError {
    GatewayError {
        kind: ErrorKind::Authentication,
        message: "Invalid admin key".to_string(),
    }
}

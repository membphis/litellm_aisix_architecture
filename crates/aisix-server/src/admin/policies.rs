use aisix_config::etcd_model::PolicyConfig;
use aisix_types::error::{ErrorKind, GatewayError};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde_json::Value;

use crate::{
    admin::{
        auth::require_admin, ensure_path_matches_body_id, ensure_valid_resource_id,
        validation::validate_admin_put_request,
    },
    app::ServerState,
};

pub async fn put_policy(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    validate_admin_put_request("policies", &payload)?;
    let policy: PolicyConfig = serde_json::from_value(payload).map_err(|error| GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: error.to_string(),
    })?;
    ensure_path_matches_body_id(&id, &policy.id)?;
    let result = admin.put_policy(&id, policy).await?;
    Ok(Json(result))
}

pub async fn get_policy(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<PolicyConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let policy = admin.get_policy(&id).await?;
    Ok(Json(policy))
}

pub async fn list_policies(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PolicyConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    let policies = admin.list_policies().await?;
    Ok(Json(policies))
}

pub async fn delete_policy(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let result = admin.delete_policy(&id).await?;
    Ok(Json(result))
}

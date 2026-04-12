use aisix_config::etcd_model::ApiKeyConfig;
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

pub async fn put_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    validate_admin_put_request("apikeys", &payload)?;
    let apikey: ApiKeyConfig = serde_json::from_value(payload).map_err(|error| GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: error.to_string(),
    })?;
    ensure_path_matches_body_id(&id, &apikey.id)?;
    let result = admin.put_apikey(&id, apikey).await?;
    Ok(Json(result))
}

pub async fn get_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ApiKeyConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let apikey = admin.get_apikey(&id).await?;
    Ok(Json(apikey))
}

pub async fn list_apikeys(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeyConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    let apikeys = admin.list_apikeys().await?;
    Ok(Json(apikeys))
}

pub async fn delete_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let result = admin.delete_apikey(&id).await?;
    Ok(Json(result))
}

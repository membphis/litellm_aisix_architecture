use aisix_config::etcd_model::ApiKeyConfig;
use aisix_types::error::GatewayError;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

use crate::{admin::{auth::require_admin, ensure_path_matches_body_id, ensure_valid_resource_id}, app::ServerState};

pub async fn put_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(apikey): Json<ApiKeyConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
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

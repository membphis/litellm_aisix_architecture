use aisix_config::etcd_model::ProviderConfig;
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

pub async fn put_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    validate_admin_put_request("providers", &payload)?;
    let provider: ProviderConfig = serde_json::from_value(payload).map_err(|error| GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: error.to_string(),
    })?;
    ensure_path_matches_body_id(&id, &provider.id)?;
    let result = admin.put_provider(&id, provider).await?;
    Ok(Json(result))
}

pub async fn get_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProviderConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let provider = admin.get_provider(&id).await?;
    Ok(Json(provider))
}

pub async fn list_providers(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    let providers = admin.list_providers().await?;
    Ok(Json(providers))
}

pub async fn delete_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let result = admin.delete_provider(&id).await?;
    Ok(Json(result))
}

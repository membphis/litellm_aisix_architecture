use aisix_config::etcd_model::ProviderConfig;
use aisix_types::error::GatewayError;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};

use crate::{
    admin::{auth::require_admin, ensure_path_matches_body_id, ensure_valid_resource_id},
    app::ServerState,
};

pub async fn put_provider(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(provider): Json<ProviderConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
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

use aisix_config::etcd_model::ModelConfig;
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

pub async fn put_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(model): Json<ModelConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_path_matches_body_id(&id, &model.id)?;
    let result = admin.put_model(&id, model).await?;
    Ok(Json(result))
}

pub async fn get_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ModelConfig>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let model = admin.get_model(&id).await?;
    Ok(Json(model))
}

pub async fn list_models(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ModelConfig>>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    let models = admin.list_models().await?;
    Ok(Json(models))
}

pub async fn delete_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_valid_resource_id(&id)?;
    let result = admin.delete_model(&id).await?;
    Ok(Json(result))
}

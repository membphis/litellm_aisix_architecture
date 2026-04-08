use aisix_config::etcd_model::ModelConfig;
use aisix_types::error::GatewayError;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

use crate::{admin::{auth::require_admin, ensure_path_matches_body_id}, app::ServerState};

pub async fn put_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(model): Json<ModelConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_path_matches_body_id(&id, &model.id)?;
    let result = admin.put_model(&id, model)?;
    Ok(Json(result))
}

use aisix_config::etcd_model::ApiKeyConfig;
use aisix_types::error::GatewayError;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

use crate::{admin::{auth::require_admin, ensure_path_matches_body_id}, app::ServerState};

pub async fn put_apikey(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(apikey): Json<ApiKeyConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_path_matches_body_id(&id, &apikey.id)?;
    let result = admin.put_apikey(&id, apikey)?;
    Ok(Json(result))
}

use aisix_config::etcd_model::ProviderConfig;
use aisix_types::error::GatewayError;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

use crate::{admin::{auth::require_admin, ensure_path_matches_body_id}, app::ServerState};

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

use aisix_config::etcd_model::PolicyConfig;
use aisix_types::error::GatewayError;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

use crate::{admin::{auth::require_admin, ensure_path_matches_body_id}, app::ServerState};

pub async fn put_policy(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(policy): Json<PolicyConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_path_matches_body_id(&id, &policy.id)?;
    let result = admin.put_policy(&id, policy)?;
    Ok(Json(result))
}

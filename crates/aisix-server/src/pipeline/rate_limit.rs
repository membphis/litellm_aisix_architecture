use aisix_core::RequestContext;
use aisix_types::error::{ErrorKind, GatewayError};

use crate::app::ServerState;

pub async fn check(ctx: &RequestContext, state: &ServerState) -> Result<impl Drop, GatewayError> {
    let provider_id = ctx
        .resolved_provider_id
        .as_deref()
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved provider missing before rate limit check".to_string(),
        })?;

    state
        .app
        .rate_limits
        .precheck(
            ctx.snapshot.as_ref(),
            &ctx.key_meta.key_id,
            ctx.request.model_name(),
            provider_id,
        )
        .await
}

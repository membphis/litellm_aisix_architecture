use aisix_core::RequestContext;

use crate::app::ServerState;

pub async fn record_success(ctx: &RequestContext, state: &ServerState) {
    let Some(usage) = ctx.usage.as_ref() else {
        return;
    };

    state
        .app
        .usage_recorder
        .record_success(&ctx.key_meta, ctx.request.model_name(), usage)
        .await;
}

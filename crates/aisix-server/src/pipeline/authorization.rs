use aisix_core::RequestContext;
use aisix_policy::access::ensure_model_allowed;
use aisix_types::error::GatewayError;

pub fn check(ctx: &RequestContext) -> Result<(), GatewayError> {
    ensure_model_allowed(&ctx.key_meta, ctx.request.model_name())
}

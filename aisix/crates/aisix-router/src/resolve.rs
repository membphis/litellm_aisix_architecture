use aisix_config::snapshot::CompiledSnapshot;
use aisix_types::error::{ErrorKind, GatewayError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub provider_id: String,
    pub upstream_model: String,
}

pub fn resolve_fixed_model(
    snapshot: &CompiledSnapshot,
    model_name: &str,
) -> Result<ResolvedTarget, GatewayError> {
    let model = snapshot
        .models_by_name
        .get(model_name)
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: format!("Unknown model '{model_name}'"),
        })?;

    Ok(ResolvedTarget {
        provider_id: model.provider_id.clone(),
        upstream_model: model.upstream_model.clone(),
    })
}

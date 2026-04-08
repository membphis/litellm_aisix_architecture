use aisix_types::{
    entities::KeyMeta,
    error::{ErrorKind, GatewayError},
};

pub fn ensure_model_allowed(meta: &KeyMeta, requested_model: &str) -> Result<(), GatewayError> {
    if meta
        .allowed_models
        .iter()
        .any(|model| model == requested_model)
    {
        Ok(())
    } else {
        Err(GatewayError {
            kind: ErrorKind::Permission,
            message: format!("Model '{requested_model}' is not allowed for this key"),
        })
    }
}

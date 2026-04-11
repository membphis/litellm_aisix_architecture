use aisix_core::AppState;
use aisix_types::{
    entities::KeyMeta,
    error::{ErrorKind, GatewayError},
};
use axum::async_trait;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts},
};
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct AuthenticatedKey {
    pub token: String,
    pub meta: KeyMeta,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedKey
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = GatewayError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = bearer_token(parts);
        let token = token?;
        let snapshot = app_state.snapshot.load();
        let meta = snapshot
            .keys_by_token
            .get(&token)
            .cloned()
            .ok_or_else(invalid_api_key)?;

        if meta
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(invalid_api_key());
        }

        Ok(Self { token, meta })
    }
}

fn bearer_token(parts: &Parts) -> Result<String, GatewayError> {
    let value = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(invalid_api_key)?;

    let (scheme, token) = value.split_once(' ').ok_or_else(invalid_api_key)?;

    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(invalid_api_key());
    }

    Ok(token.to_string())
}

fn invalid_api_key() -> GatewayError {
    GatewayError {
        kind: ErrorKind::Authentication,
        message: "Invalid API key".to_string(),
    }
}

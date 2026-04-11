use axum::{extract::State, http::StatusCode};

use crate::app::ServerState;

pub async fn health() -> StatusCode {
    StatusCode::OK
}

pub async fn ready(State(state): State<ServerState>) -> StatusCode {
    if state.app.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

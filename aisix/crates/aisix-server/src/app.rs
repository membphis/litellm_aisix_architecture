use std::net::SocketAddr;

use anyhow::Context;
use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use axum::{Router, routing::{get, post, put}};

use crate::{admin, handlers, health};

#[derive(Debug, Clone)]
pub struct ServerState {
    pub app: AppState,
    pub providers: ProviderRegistry,
    pub admin: Option<admin::AdminState>,
}

impl axum::extract::FromRef<ServerState> for AppState {
    fn from_ref(input: &ServerState) -> Self {
        input.app.clone()
    }
}

pub fn build_router(state: ServerState) -> Router {
    let router = Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/v1/chat/completions", post(handlers::chat::chat_completions))
        .route("/v1/embeddings", post(handlers::embeddings::embeddings));

    let router = if state.admin.is_some() {
        router
            .route("/admin/providers/:id", put(admin::providers::put_provider))
            .route("/admin/models/:id", put(admin::models::put_model))
            .route("/admin/apikeys/:id", put(admin::apikeys::put_apikey))
            .route("/admin/policies/:id", put(admin::policies::put_policy))
    } else {
        router
    };

    router.with_state(state)
}

pub async fn serve(state: AppState, listen: &str, admin: Option<admin::AdminState>) -> anyhow::Result<()> {
    let address: SocketAddr = listen
        .parse()
        .with_context(|| format!("invalid listen address: {listen}"))?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    axum::serve(
        listener,
        build_router(ServerState {
            app: state,
            providers: ProviderRegistry::default(),
            admin,
        }),
    )
        .await
        .map_err(Into::into)
}

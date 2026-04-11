use std::net::SocketAddr;

use aisix_core::AppState;
use aisix_providers::ProviderRegistry;
use anyhow::Context;
use axum::{
    routing::{get, post},
    Router,
};
use tracing::info;

use crate::{admin, handlers, health, ui};

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
    build_data_plane_router(state.clone()).merge(build_admin_router(state))
}

pub fn build_data_plane_router(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route(
            "/v1/chat/completions",
            post(handlers::chat::chat_completions),
        )
        .route("/v1/messages", post(handlers::anthropic::messages))
        .route("/v1/embeddings", post(handlers::embeddings::embeddings))
        .with_state(state)
}

pub fn build_admin_router(state: ServerState) -> Router {
    let router = Router::new();

    let router = if state.admin.is_some() {
        router
            .route("/admin/providers", get(admin::providers::list_providers))
            .route(
                "/admin/providers/:id",
                get(admin::providers::get_provider)
                    .put(admin::providers::put_provider)
                    .delete(admin::providers::delete_provider),
            )
            .route("/admin/models", get(admin::models::list_models))
            .route(
                "/admin/models/:id",
                get(admin::models::get_model)
                    .put(admin::models::put_model)
                    .delete(admin::models::delete_model),
            )
            .route("/admin/apikeys", get(admin::apikeys::list_apikeys))
            .route(
                "/admin/apikeys/:id",
                get(admin::apikeys::get_apikey)
                    .put(admin::apikeys::put_apikey)
                    .delete(admin::apikeys::delete_apikey),
            )
            .route("/admin/policies", get(admin::policies::list_policies))
            .route(
                "/admin/policies/:id",
                get(admin::policies::get_policy)
                    .put(admin::policies::put_policy)
                    .delete(admin::policies::delete_policy),
            )
            .route("/ui", get(ui::admin_ui_index))
            .route("/ui/app.js", get(ui::admin_ui_app_js))
    } else {
        router
    };

    router.with_state(state)
}

pub async fn serve(
    state: AppState,
    listen: &str,
    admin: Option<admin::AdminState>,
) -> anyhow::Result<()> {
    let admin_enabled = admin.is_some();
    let router = build_router(ServerState {
        app: state,
        providers: ProviderRegistry::default(),
        admin,
    });

    serve_router(router, listen, admin_enabled).await
}

pub async fn serve_data_plane(state: AppState, listen: &str) -> anyhow::Result<()> {
    let router = build_data_plane_router(ServerState {
        app: state,
        providers: ProviderRegistry::default(),
        admin: None,
    });

    serve_router(router, listen, false).await
}

pub async fn serve_admin(
    state: AppState,
    listen: &str,
    admin: admin::AdminState,
) -> anyhow::Result<()> {
    let router = build_admin_router(ServerState {
        app: state,
        providers: ProviderRegistry::default(),
        admin: Some(admin),
    });

    serve_router(router, listen, true).await
}

async fn serve_router(router: Router, listen: &str, admin_enabled: bool) -> anyhow::Result<()> {
    let address: SocketAddr = listen
        .parse()
        .with_context(|| format!("invalid listen address: {listen}"))?;
    log_binding_http_listener(address, admin_enabled);
    let listener = tokio::net::TcpListener::bind(address).await?;
    log_gateway_listening(address, admin_enabled);

    axum::serve(listener, router).await.map_err(Into::into)
}

fn log_binding_http_listener(address: SocketAddr, admin_enabled: bool) {
    info!(listen = %address, admin_enabled, "binding http listener");
}

fn log_gateway_listening(address: SocketAddr, admin_enabled: bool) {
    info!(listen = %address, admin_enabled, "gateway listening");
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, Mutex},
    };

    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    use super::{log_binding_http_listener, log_gateway_listening};

    #[test]
    fn server_logs_binding_and_listening_state() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4000);
        let output = capture_logs(|| {
            log_binding_http_listener(address, true);
            log_gateway_listening(address, true);
        });

        assert!(output.contains("binding http listener"));
        assert!(output.contains("gateway listening"));
        assert!(output.contains("127.0.0.1:4000"));
        assert!(output.contains("admin_enabled=true"));
    }

    fn capture_logs(run: impl FnOnce()) -> String {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_writer(TestWriter(buffer.clone()))
            .finish();

        with_default(subscriber, run);

        let captured = buffer.lock().unwrap().clone();
        String::from_utf8(captured).unwrap()
    }

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for TestWriter {
        type Writer = TestWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            TestWriterGuard(self.0.clone())
        }
    }

    struct TestWriterGuard(Arc<Mutex<Vec<u8>>>);

    impl io::Write for TestWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

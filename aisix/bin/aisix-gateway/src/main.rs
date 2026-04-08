use aisix_config::startup::load_from_path;
use anyhow::{anyhow, Context};
use std::path::PathBuf;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = resolve_config_path(std::env::args().nth(1))?;
    let config = load_from_path(
        config_path
            .to_str()
            .context("config path must be valid utf-8")?,
    )?;

    aisix_observability::tracing_init::init(&config.log.level)?;
    log_gateway_start(&config_path, &config.log.level);
    log_metrics_configuration(&config.server.metrics_listen);

    let state = aisix_runtime::bootstrap::bootstrap(&config)
        .await
        .inspect_err(|error| log_startup_failure("runtime bootstrap", error))?;
    info!("runtime bootstrap complete");
    let admin = aisix_server::admin::AdminState::from_startup_config(&config)
        .await
        .inspect_err(|error| log_startup_failure("admin initialization", error))?;
    info!(admin_enabled = admin.is_some(), "admin initialization complete");
    log_gateway_starting_http_server(&config.server.listen, admin.is_some());

    aisix_server::app::serve(state, &config.server.listen, admin)
        .await
        .inspect_err(|error| log_startup_failure("http server", error))
}

fn log_gateway_start(config_path: &std::path::Path, log_level: &str) {
    info!(config_path = %config_path.display(), log_level, "gateway starting");
}

fn log_gateway_starting_http_server(listen: &str, admin_enabled: bool) {
    info!(listen, admin_enabled, "starting http server");
}

fn log_metrics_configuration(metrics_listen: &str) {
    info!(metrics_listen, "metrics endpoint configured");
}

fn log_startup_failure(stage: &str, error_message: &dyn std::fmt::Display) {
    error!(stage, error = %error_message, "startup failed");
}

fn resolve_config_path(cli_path: Option<String>) -> anyhow::Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(PathBuf::from(path));
    }

    let current_dir_candidate = PathBuf::from("config/aisix-gateway.example.yaml");
    if current_dir_candidate.exists() {
        return Ok(current_dir_candidate);
    }

    if let Some(exe_dir) = std::env::current_exe()?.parent() {
        let exe_dir_candidate = exe_dir.join("../config/aisix-gateway.example.yaml");
        if exe_dir_candidate.exists() {
            return Ok(normalize_path(exe_dir_candidate));
        }
    }

    Err(anyhow!(
        "could not locate startup config; tried ./config/aisix-gateway.example.yaml and ../config/aisix-gateway.example.yaml relative to the executable"
    ))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    match path.canonicalize() {
        Ok(path) => path,
        Err(_) => path,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::Path,
        sync::{Arc, Mutex},
    };

    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    use super::{
        log_gateway_start, log_gateway_starting_http_server, log_metrics_configuration,
        log_startup_failure,
    };

    #[test]
    fn startup_logs_include_config_path_and_level() {
        let output = capture_logs(|| log_gateway_start(Path::new("config/example.yaml"), "info"));

        assert!(output.contains("gateway starting"));
        assert!(output.contains("config/example.yaml"));
        assert!(output.contains("info"));
    }

    #[test]
    fn startup_logs_include_server_handoff_fields() {
        let output = capture_logs(|| log_gateway_starting_http_server("0.0.0.0:4000", true));

        assert!(output.contains("starting http server"));
        assert!(output.contains("0.0.0.0:4000"));
        assert!(output.contains("admin_enabled=true"));
    }

    #[test]
    fn startup_logs_include_metrics_configuration() {
        let output = capture_logs(|| log_metrics_configuration("0.0.0.0:9090"));

        assert!(output.contains("metrics endpoint configured"));
        assert!(output.contains("0.0.0.0:9090"));
    }

    #[test]
    fn startup_failure_log_includes_stage_and_error() {
        let error = anyhow::anyhow!("redis unavailable");
        let output = capture_logs(|| log_startup_failure("runtime bootstrap", &error));

        assert!(output.contains("startup failed"));
        assert!(output.contains("runtime bootstrap"));
        assert!(output.contains("redis unavailable"));
        assert!(output.contains("ERROR"));
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

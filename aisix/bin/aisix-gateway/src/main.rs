use aisix_config::startup::load_from_path;
use anyhow::{anyhow, Context};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = resolve_config_path(std::env::args().nth(1))?;
    let config = load_from_path(
        config_path
            .to_str()
            .context("config path must be valid utf-8")?,
    )?;

    aisix_observability::tracing_init::init(&config.log.level)?;

    let state = aisix_runtime::bootstrap::bootstrap(&config).await?;
    let admin = aisix_server::admin::AdminState::from_startup_config(&config).await?;

    aisix_server::app::serve(state, &config.server.listen, admin).await
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

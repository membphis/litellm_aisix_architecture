use aisix_config::startup::{load_from_path, CacheDefaultMode};
use std::fs;

#[test]
fn loads_example_startup_config() {
    let path = format!(
        "{}/../../config/aisix-gateway.example.yaml",
        env!("CARGO_MANIFEST_DIR")
    );
    let config = load_from_path(&path).expect("example config should load");

    assert_eq!(config.server.listen, "0.0.0.0:4000");
    assert_eq!(config.etcd.prefix, "/aisix");
    assert_eq!(config.cache.default, CacheDefaultMode::Disabled);
    assert!(config
        .deployment
        .admin
        .admin_keys
        .first()
        .is_some_and(|key| !key.key.is_empty()));
}

#[test]
fn missing_cache_section_defaults_to_disabled() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join(format!("aisix-startup-config-{}.yaml", std::process::id()));

    fs::write(
        &path,
        r#"server:
  listen: "0.0.0.0:4000"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8
etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000
redis:
  url: "redis://127.0.0.1:6379"
log:
  level: "info"
runtime:
  worker_threads: 0
deployment:
  admin:
    enabled: false
"#,
    )
    .expect("temporary config should be written");

    let config = load_from_path(path.to_str().expect("temp path should be valid utf-8"))
        .expect("config without cache section should load");

    assert_eq!(config.cache.default, CacheDefaultMode::Disabled);

    fs::remove_file(path).expect("temporary config should be cleaned up");
}

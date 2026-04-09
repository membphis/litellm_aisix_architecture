mod support;

use aisix_config::startup::{
    AdminConfig, DeploymentConfig, EtcdConfig, LogConfig, RedisConfig, RuntimeConfig,
    ServerConfig, StartupConfig,
};
use serde_json::json;

#[tokio::test]
async fn bootstrap_fails_when_etcd_is_unreachable() {
    let config = StartupConfig {
        server: ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: EtcdConfig {
            endpoints: vec!["127.0.0.1:1".to_string()],
            prefix: "/aisix".to_string(),
            dial_timeout_ms: 100,
        },
        redis: RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: LogConfig {
            level: "info".to_string(),
        },
        runtime: RuntimeConfig { worker_threads: 1 },
        deployment: DeploymentConfig {
            admin: AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    };

    let error = aisix_runtime::bootstrap::bootstrap(&config)
        .await
        .expect_err("startup should fail");

    assert!(error.to_string().contains("etcd"));
}

#[tokio::test]
async fn bootstrap_loads_initial_snapshot_from_etcd() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    let expected_revision = harness
        .put_json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "openai",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("model fixture should be written");
    harness
        .put_json(
            "/aisix/apikeys/demo",
            &json!({
                "id": "demo",
                "key": "sk-demo",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("apikey fixture should be written");

    let config = StartupConfig {
        server: ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: LogConfig {
            level: "info".to_string(),
        },
        runtime: RuntimeConfig { worker_threads: 1 },
        deployment: DeploymentConfig {
            admin: AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    };

    let state = aisix_runtime::bootstrap::bootstrap(&config)
        .await
        .expect("startup should succeed");
    let snapshot = state.snapshot.load();

    assert_eq!(snapshot.revision, expected_revision + 2);
    assert!(snapshot.providers_by_id.contains_key("openai"));
    assert!(snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(snapshot.keys_by_token.contains_key("sk-demo"));
}

#[tokio::test]
async fn bootstrap_loads_valid_subset_when_etcd_contains_dependency_invalid_resource() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "openai",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("model fixture should be written");
    harness
        .put_json(
            "/aisix/apikeys/demo",
            &json!({
                "id": "demo",
                "key": "sk-demo",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("apikey fixture should be written");
    harness
        .put_json(
            "/aisix/models/broken-model",
            &json!({
                "id": "broken-model",
                "provider_id": "missing-provider",
                "upstream_model": "broken-model",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("broken model fixture should be written");

    let config = StartupConfig {
        server: ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: LogConfig {
            level: "info".to_string(),
        },
        runtime: RuntimeConfig { worker_threads: 1 },
        deployment: DeploymentConfig {
            admin: AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    };

    let state = aisix_runtime::bootstrap::bootstrap(&config)
        .await
        .expect("startup should succeed");
    let snapshot = state.snapshot.load();

    assert!(snapshot.providers_by_id.contains_key("openai"));
    assert!(snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(snapshot.keys_by_token.contains_key("sk-demo"));
    assert!(!snapshot.models_by_name.contains_key("broken-model"));
}

#[tokio::test]
async fn bootstrap_stops_watcher_when_last_state_is_dropped() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "openai",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("model fixture should be written");

    let config = StartupConfig {
        server: ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: LogConfig {
            level: "info".to_string(),
        },
        runtime: RuntimeConfig { worker_threads: 1 },
        deployment: DeploymentConfig {
            admin: AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    };

    let state = aisix_runtime::bootstrap::bootstrap(&config)
        .await
        .expect("startup should succeed");
    let snapshot = state.snapshot.clone();

    drop(state);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    harness.pause().expect("etcd should pause after state drop");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    harness.unpause().expect("etcd should unpause after state drop");

    harness
        .put_json(
            "/aisix/apikeys/demo",
            &json!({
                "id": "demo",
                "key": "sk-demo",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": null,
                "rate_limit": null
            }),
        )
        .await
        .expect("apikey fixture should be written");

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    assert!(!snapshot.load().keys_by_token.contains_key("sk-demo"));
}

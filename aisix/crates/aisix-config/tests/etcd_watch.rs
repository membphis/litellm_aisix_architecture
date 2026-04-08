mod support;

use std::time::{Duration, Instant};

use aisix_config::{
    compile::compile_snapshot,
    watcher::{initial_snapshot_handle, spawn_snapshot_watcher},
};
use serde_json::json;

#[tokio::test]
async fn watcher_reloads_snapshot_after_put() {
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

    let snapshot = initial_snapshot_handle(
        compile_snapshot(vec![], vec![], vec![], vec![], 0).expect("empty snapshot should compile"),
    );
    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

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

    wait_until(|| snapshot.load().models_by_name.contains_key("gpt-4o-mini")).await;

    watcher.abort();
}

#[tokio::test]
async fn watcher_keeps_previous_snapshot_when_reload_fails() {
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

    let initial = aisix_config::watcher::load_initial_snapshot(&aisix_config::startup::StartupConfig {
        server: aisix_config::startup::ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: aisix_config::startup::RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: aisix_config::startup::LogConfig {
            level: "info".to_string(),
        },
        runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
        deployment: aisix_config::startup::DeploymentConfig {
            admin: aisix_config::startup::AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    })
    .await
    .expect("initial snapshot should load");
    let snapshot = initial_snapshot_handle(initial);
    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

    harness
        .delete("/aisix/providers/openai")
        .await
        .expect("provider fixture should be deleted");

    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(snapshot.load().models_by_name.contains_key("gpt-4o-mini"));
    assert!(snapshot.load().keys_by_token.contains_key("sk-demo"));

    watcher.abort();
}

#[tokio::test]
async fn watcher_recovers_from_compacted_revision_with_full_reload() {
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

    let snapshot = initial_snapshot_handle(
        aisix_config::watcher::load_initial_snapshot(&aisix_config::startup::StartupConfig {
            server: aisix_config::startup::ServerConfig {
                listen: "127.0.0.1:0".to_string(),
                metrics_listen: "127.0.0.1:0".to_string(),
                request_body_limit_mb: 1,
            },
            etcd: harness.config(),
            redis: aisix_config::startup::RedisConfig {
                url: "redis://127.0.0.1:1".to_string(),
            },
            log: aisix_config::startup::LogConfig {
                level: "info".to_string(),
            },
            runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
            deployment: aisix_config::startup::DeploymentConfig {
                admin: aisix_config::startup::AdminConfig {
                    enabled: false,
                    admin_keys: vec![],
                },
            },
        })
        .await
        .expect("initial snapshot should load"),
    );

    let compacted_revision = harness
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
        .compact(compacted_revision)
        .await
        .expect("history should compact");

    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

    wait_until(|| snapshot.load().keys_by_token.contains_key("sk-demo")).await;

    watcher.abort();
}

#[tokio::test]
async fn watcher_reconnects_after_transient_reload_failure() {
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

    let initial = aisix_config::watcher::load_initial_snapshot(&aisix_config::startup::StartupConfig {
        server: aisix_config::startup::ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd: harness.config(),
        redis: aisix_config::startup::RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: aisix_config::startup::LogConfig {
            level: "info".to_string(),
        },
        runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
        deployment: aisix_config::startup::DeploymentConfig {
            admin: aisix_config::startup::AdminConfig {
                enabled: false,
                admin_keys: vec![],
            },
        },
    })
    .await
    .expect("initial snapshot should load");
    let snapshot = initial_snapshot_handle(initial);
    let watcher = spawn_snapshot_watcher(harness.config(), snapshot.clone())
        .await
        .expect("watcher should start");

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

    harness.pause().expect("etcd should pause");
    tokio::time::sleep(Duration::from_millis(200)).await;
    harness.unpause().expect("etcd should unpause");

    wait_until(|| snapshot.load().keys_by_token.contains_key("sk-demo")).await;

    watcher.abort();
}

async fn wait_until<F>(mut check: F)
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    panic!("condition not met before timeout");
}

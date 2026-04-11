mod support;

use aisix_config::{
    etcd::EtcdStore,
    etcd_model::{ProviderAuth, ProviderConfig, ProviderKind},
};

fn provider(id: &str, base_url: &str) -> ProviderConfig {
    ProviderConfig {
        id: id.to_string(),
        kind: ProviderKind::OpenAi,
        base_url: base_url.to_string(),
        auth: ProviderAuth {
            secret_ref: format!("env:{}_API_KEY", id.to_ascii_uppercase()),
        },
        policy_id: None,
        rate_limit: None,
        cache: None,
    }
}

#[tokio::test]
async fn gets_json_from_live_etcd() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &provider("openai", "https://api.openai.com"),
        )
        .await
        .expect("fixture provider should be written");

    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let stored: Option<ProviderConfig> = store
        .get_json("/aisix", "providers", "openai")
        .await
        .expect("get should succeed");
    let stored = stored.expect("provider should exist");

    assert_eq!(stored.id, "openai");
    assert_eq!(stored.base_url, "https://api.openai.com");
}

#[tokio::test]
async fn lists_json_from_live_etcd_collection() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &provider("openai", "https://api.openai.com"),
        )
        .await
        .expect("openai fixture should be written");
    harness
        .put_json(
            "/aisix/providers/anthropic",
            &provider("anthropic", "https://api.anthropic.com"),
        )
        .await
        .expect("anthropic fixture should be written");

    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let mut providers: Vec<ProviderConfig> = store
        .list_json("/aisix/", "providers")
        .await
        .expect("list should succeed");
    providers.sort_by(|left, right| left.id.cmp(&right.id));

    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].id, "anthropic");
    assert_eq!(providers[0].base_url, "https://api.anthropic.com");
    assert_eq!(providers[1].id, "openai");
    assert_eq!(providers[1].base_url, "https://api.openai.com");
}

#[tokio::test]
async fn list_json_excludes_sibling_prefixes() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &provider("openai", "https://api.openai.com"),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/providers_backup/openai",
            &provider("backup", "https://backup.example.com"),
        )
        .await
        .expect("sibling fixture should be written");

    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let providers: Vec<ProviderConfig> = store
        .list_json("/aisix", "providers")
        .await
        .expect("list should succeed");

    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].id, "openai");
}

#[tokio::test]
async fn missing_get_returns_none() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let provider = store
        .get_json::<ProviderConfig>("/aisix", "providers", "missing")
        .await
        .expect("missing get should succeed");

    assert!(provider.is_none());
}

#[tokio::test]
async fn delete_reports_whether_key_existed() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    harness
        .put_json(
            "/aisix/providers/openai",
            &provider("openai", "https://api.openai.com"),
        )
        .await
        .expect("fixture provider should be written");

    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let deleted = store
        .delete("/aisix", "providers", "openai")
        .await
        .expect("delete should succeed");
    let missing = store
        .delete("/aisix", "providers", "missing")
        .await
        .expect("missing delete should still succeed");

    assert_eq!(deleted.key, "/aisix/providers/openai");
    assert!(deleted.existed);
    assert!(deleted.revision > 0);

    assert_eq!(missing.key, "/aisix/providers/missing");
    assert!(!missing.existed);
}

#[tokio::test]
async fn put_json_returns_key_and_revision() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    let mut store = EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let write = store
        .put_json(
            "/aisix",
            "providers",
            "openai",
            &provider("openai", "https://api.openai.com"),
        )
        .await
        .expect("put should succeed");

    assert_eq!(write.key, "/aisix/providers/openai");
    assert!(write.revision > 0);
}

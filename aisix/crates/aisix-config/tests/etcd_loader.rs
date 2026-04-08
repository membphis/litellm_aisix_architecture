mod support;

use aisix_config::loader::{compile_snapshot_from_entries, EtcdEntry};
use serde_json::json;

#[test]
fn joins_collection_paths_under_prefix() {
    assert_eq!(
        aisix_config::etcd::resource_key("/aisix", "providers", "openai"),
        "/aisix/providers/openai"
    );
}

#[test]
fn normalizes_trailing_slashes_in_prefix() {
    assert_eq!(
        aisix_config::etcd::resource_key("/aisix/", "models", "gpt-4o-mini"),
        "/aisix/models/gpt-4o-mini"
    );
}

#[test]
fn compiles_snapshot_from_etcd_entries() {
    let entries = vec![
        EtcdEntry::json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        ),
        EtcdEntry::json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "openai",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        ),
        EtcdEntry::json(
            "/aisix/apikeys/demo",
            &json!({
                "id": "demo",
                "key": "sk-demo",
                "allowed_models": ["gpt-4o-mini"],
                "policy_id": null,
                "rate_limit": null
            }),
        ),
    ];

    let snapshot =
        compile_snapshot_from_entries("/aisix/", &entries, 9).expect("snapshot should compile");

    assert_eq!(snapshot.revision, 9);
    assert!(snapshot.providers_by_id.contains_key("openai"));
    assert!(snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(snapshot.keys_by_token.contains_key("sk-demo"));
}

#[test]
fn rejects_invalid_model_reference_from_etcd_entries() {
    let entries = vec![
        EtcdEntry::json(
            "/aisix/providers/openai",
            &json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "https://api.openai.com",
                "auth": { "secret_ref": "env:OPENAI_API_KEY" },
                "policy_id": null,
                "rate_limit": null
            }),
        ),
        EtcdEntry::json(
            "/aisix/models/gpt-4o-mini",
            &json!({
                "id": "gpt-4o-mini",
                "provider_id": "missing-provider",
                "upstream_model": "gpt-4o-mini",
                "policy_id": null,
                "rate_limit": null
            }),
        ),
    ];

    let error = compile_snapshot_from_entries("/aisix", &entries, 2)
        .expect_err("invalid references should fail");

    assert_eq!(error, "missing provider reference: missing-provider");
}

#[test]
fn rejects_key_and_payload_id_mismatches() {
    let entries = vec![EtcdEntry::json(
        "/aisix/providers/openai",
        &json!({
            "id": "anthropic",
            "kind": "openai",
            "base_url": "https://api.openai.com",
            "auth": { "secret_ref": "env:OPENAI_API_KEY" },
            "policy_id": null,
            "rate_limit": null
        }),
    )];

    let error = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect_err("key and payload id mismatches should fail");

    assert_eq!(
        error,
        "etcd key/body id mismatch for provider at /aisix/providers/openai: expected openai, got anthropic"
    );
}

#[test]
fn includes_key_and_type_in_decode_errors() {
    let entries = vec![EtcdEntry {
        key: "/aisix/models/gpt-4o-mini".to_string(),
        value: br#"{"id":"gpt-4o-mini","provider_id":123}"#.to_vec(),
    }];

    let error = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect_err("decode failures should include key context");

    assert!(error.starts_with("failed to decode model at /aisix/models/gpt-4o-mini: "));
}

#[test]
fn rejects_keys_outside_normalized_prefix() {
    let entries = vec![EtcdEntry::json(
        "/other/providers/openai",
        &json!({
            "id": "openai",
            "kind": "openai",
            "base_url": "https://api.openai.com",
            "auth": { "secret_ref": "env:OPENAI_API_KEY" },
            "policy_id": null,
            "rate_limit": null
        }),
    )];

    let error = compile_snapshot_from_entries("/aisix/", &entries, 1)
        .expect_err("keys outside prefix should fail");

    assert_eq!(
        error,
        "invalid etcd key outside prefix: /other/providers/openai"
    );
}

#[test]
fn rejects_malformed_keys_without_resource_id() {
    let entries = vec![EtcdEntry::json(
        "/aisix/providers",
        &json!({
            "id": "openai",
            "kind": "openai",
            "base_url": "https://api.openai.com",
            "auth": { "secret_ref": "env:OPENAI_API_KEY" },
            "policy_id": null,
            "rate_limit": null
        }),
    )];

    let error = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect_err("malformed keys should fail");

    assert_eq!(error, "invalid etcd key: /aisix/providers");
}

#[test]
fn rejects_unsupported_collection() {
    let entries = vec![EtcdEntry::json(
        "/aisix/tenants/demo",
        &json!({
            "id": "demo"
        }),
    )];

    let error = compile_snapshot_from_entries("/aisix", &entries, 1)
        .expect_err("unsupported collections should fail");

    assert_eq!(error, "unsupported etcd collection: tenants");
}

#[tokio::test]
async fn loads_prefix_entries_and_revision_from_live_etcd() {
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
        .expect("fixture provider should be written");

    let mut store = aisix_config::etcd::EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");
    let (entries, revision) = store
        .load_prefix("/aisix")
        .await
        .expect("prefix load should succeed");

    assert_eq!(revision, expected_revision);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "/aisix/providers/openai");
}

#[tokio::test]
async fn writes_json_to_live_etcd_with_normalized_resource_path() {
    let harness = support::etcd::EtcdHarness::start()
        .await
        .expect("test etcd should start");
    let mut store = aisix_config::etcd::EtcdStore::connect(&harness.config())
        .await
        .expect("store should connect");

    let write = store
        .put_json(
            "/aisix/",
            "providers",
            "openai",
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
        .expect("put should succeed");
    let (entries, revision) = store
        .load_prefix("/aisix")
        .await
        .expect("prefix load should succeed");

    assert_eq!(write.key, "/aisix/providers/openai");
    assert_eq!(write.revision, revision);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, write.key);
}

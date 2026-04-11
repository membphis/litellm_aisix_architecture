use aisix_config::compile::compile_snapshot;
use aisix_config::etcd_model::{
    ApiKeyConfig, CacheMode, CachePolicyConfig, ModelConfig, PolicyConfig, ProviderAuth,
    ProviderConfig, ProviderKind, RateLimitConfig,
};

fn provider() -> ProviderConfig {
    ProviderConfig {
        id: "provider-1".to_string(),
        kind: ProviderKind::OpenAi,
        base_url: "https://api.openai.com/v1".to_string(),
        auth: ProviderAuth {
            secret_ref: "secret/openai".to_string(),
        },
        policy_id: Some("policy-1".to_string()),
        rate_limit: None,
        cache: None,
    }
}

fn model() -> ModelConfig {
    ModelConfig {
        id: "gpt-4o-mini".to_string(),
        provider_id: "provider-1".to_string(),
        upstream_model: "gpt-4o-mini".to_string(),
        policy_id: Some("policy-1".to_string()),
        rate_limit: None,
        cache: None,
    }
}

fn policy() -> PolicyConfig {
    PolicyConfig {
        id: "policy-1".to_string(),
        rate_limit: RateLimitConfig {
            rpm: Some(100),
            tpm: Some(1000),
            concurrency: Some(10),
        },
    }
}

fn api_key() -> ApiKeyConfig {
    ApiKeyConfig {
        id: "key-1".to_string(),
        key: "sk-test".to_string(),
        allowed_models: vec!["gpt-4o-mini".to_string()],
        policy_id: Some("policy-1".to_string()),
        rate_limit: Some(RateLimitConfig {
            rpm: Some(7),
            tpm: Some(70),
            concurrency: Some(3),
        }),
    }
}

#[test]
fn compile_snapshot_prefers_inline_key_limits() {
    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key()],
        vec![policy()],
        42,
    )
    .expect("snapshot should compile");

    let snapshot = report.snapshot;

    let meta = snapshot
        .keys_by_token
        .get("sk-test")
        .expect("compiled key metadata should exist");
    assert_eq!(meta.key_id, "key-1");
    assert_eq!(meta.allowed_models, ["gpt-4o-mini"]);

    let limits = snapshot
        .key_limits
        .get("key-1")
        .expect("resolved key limits should exist");
    assert_eq!(
        (limits.rpm, limits.tpm, limits.concurrency),
        (Some(7), Some(70), Some(3))
    );
    assert_eq!(snapshot.revision, 42);
}

#[test]
fn compile_snapshot_merges_partial_inline_limits_with_policy() {
    let mut partial_key = api_key();
    partial_key.rate_limit = Some(RateLimitConfig {
        rpm: Some(7),
        tpm: None,
        concurrency: None,
    });

    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![partial_key],
        vec![policy()],
        99,
    )
    .expect("partial inline limits should inherit policy values");

    let snapshot = report.snapshot;

    let limits = snapshot
        .key_limits
        .get("key-1")
        .expect("resolved key limits should exist");
    assert_eq!(
        (limits.rpm, limits.tpm, limits.concurrency),
        (Some(7), Some(1000), Some(10))
    );
}

#[test]
fn compile_snapshot_resolves_provider_and_model_limits() {
    let mut inline_provider = provider();
    inline_provider.rate_limit = Some(RateLimitConfig {
        rpm: Some(11),
        tpm: Some(110),
        concurrency: Some(4),
    });

    let report = compile_snapshot(
        vec![inline_provider],
        vec![model()],
        vec![api_key()],
        vec![policy()],
        7,
    )
    .expect("snapshot with provider/model limits should compile");

    let snapshot = report.snapshot;

    let provider_limits = snapshot
        .provider_limits
        .get("provider-1")
        .expect("provider limits should exist");
    assert_eq!(
        (
            provider_limits.rpm,
            provider_limits.tpm,
            provider_limits.concurrency,
        ),
        (Some(11), Some(110), Some(4))
    );

    let model_limits = snapshot
        .model_limits
        .get("gpt-4o-mini")
        .expect("model limits should exist");
    assert_eq!(
        (model_limits.rpm, model_limits.tpm, model_limits.concurrency),
        (Some(100), Some(1000), Some(10))
    );
}

#[test]
fn compile_snapshot_defaults_missing_cache_to_inherit() {
    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key()],
        vec![policy()],
        1,
    )
    .expect("snapshot should compile");

    assert_eq!(
        report.snapshot.provider_cache_modes.get("provider-1"),
        Some(&CacheMode::Inherit)
    );
    assert_eq!(
        report.snapshot.model_cache_modes.get("gpt-4o-mini"),
        Some(&CacheMode::Inherit)
    );
}

#[test]
fn compile_snapshot_records_explicit_cache_modes() {
    let mut provider = provider();
    provider.cache = Some(CachePolicyConfig {
        mode: CacheMode::Enabled,
    });

    let mut model = model();
    model.cache = Some(CachePolicyConfig {
        mode: CacheMode::Disabled,
    });

    let report = compile_snapshot(vec![provider], vec![model], vec![], vec![policy()], 1)
        .expect("snapshot should compile");

    assert_eq!(
        report.snapshot.provider_cache_modes.get("provider-1"),
        Some(&CacheMode::Enabled)
    );
    assert_eq!(
        report.snapshot.model_cache_modes.get("gpt-4o-mini"),
        Some(&CacheMode::Disabled)
    );
}

#[test]
fn compile_snapshot_rejects_duplicate_plaintext_api_keys() {
    let mut second_key = api_key();
    second_key.id = "key-2".to_string();

    let error = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key(), second_key],
        vec![policy()],
        1,
    )
    .expect_err("duplicate plaintext keys should be rejected");

    assert_eq!(error, "duplicate api key token");
}

#[test]
fn compile_snapshot_rejects_duplicate_policy_ids() {
    let mut duplicate_policy = policy();
    duplicate_policy.rate_limit = RateLimitConfig {
        rpm: Some(1),
        tpm: Some(2),
        concurrency: Some(3),
    };

    let error = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key()],
        vec![policy(), duplicate_policy],
        1,
    )
    .expect_err("duplicate policy ids should be rejected");

    assert!(error.contains("duplicate policy id: policy-1"));
}

#[test]
fn compile_snapshot_rejects_duplicate_provider_ids() {
    let mut duplicate_provider = provider();
    duplicate_provider.base_url = "https://duplicate.example/v1".to_string();

    let error = compile_snapshot(
        vec![provider(), duplicate_provider],
        vec![model()],
        vec![api_key()],
        vec![policy()],
        1,
    )
    .expect_err("duplicate provider ids should be rejected");

    assert!(error.contains("duplicate provider id: provider-1"));
}

#[test]
fn compile_snapshot_rejects_duplicate_model_ids() {
    let mut duplicate_model = model();
    duplicate_model.upstream_model = "another-upstream".to_string();

    let error = compile_snapshot(
        vec![provider()],
        vec![model(), duplicate_model],
        vec![api_key()],
        vec![policy()],
        1,
    )
    .expect_err("duplicate model ids should be rejected");

    assert!(error.contains("duplicate model id: gpt-4o-mini"));
}

#[test]
fn compile_snapshot_rejects_duplicate_api_key_ids() {
    let mut second_key = api_key();
    second_key.key = "sk-second".to_string();

    let error = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key(), second_key],
        vec![policy()],
        1,
    )
    .expect_err("duplicate api key ids should be rejected");

    assert!(error.contains("duplicate api key id: key-1"));
}

#[test]
fn compile_snapshot_skips_api_keys_with_unknown_allowed_model() {
    let mut key = api_key();
    key.allowed_models = vec!["missing-model".to_string()];

    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![key],
        vec![policy()],
        1,
    )
    .expect("unknown allowed model should be skipped");

    assert!(!report.snapshot.keys_by_token.contains_key("sk-test"));
    assert!(!report.snapshot.apikeys_by_id.contains_key("key-1"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "api key");
    assert_eq!(report.issues[0].id, "key-1");
    assert_eq!(
        report.issues[0].reason,
        "missing model reference: missing-model"
    );
}

#[test]
fn compile_snapshot_skips_models_with_missing_provider_references() {
    let mut broken_model = model();
    broken_model.id = "broken-model".to_string();
    broken_model.provider_id = "missing-provider".to_string();

    let report = compile_snapshot(
        vec![provider()],
        vec![model(), broken_model],
        vec![],
        vec![policy()],
        11,
    )
    .expect("snapshot should compile by skipping invalid models");

    assert!(report.snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(!report.snapshot.models_by_name.contains_key("broken-model"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "model");
    assert_eq!(report.issues[0].id, "broken-model");
    assert_eq!(
        report.issues[0].reason,
        "missing provider reference: missing-provider"
    );
}

#[test]
fn compile_snapshot_skips_api_keys_with_unknown_policy() {
    let mut key = api_key();
    key.policy_id = Some("missing-policy".to_string());
    key.rate_limit = None;

    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![key],
        vec![policy()],
        1,
    )
    .expect("unknown api key policy should be skipped");

    assert!(!report.snapshot.keys_by_token.contains_key("sk-test"));
    assert!(!report.snapshot.apikeys_by_id.contains_key("key-1"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "api key");
    assert_eq!(report.issues[0].id, "key-1");
    assert_eq!(
        report.issues[0].reason,
        "missing policy reference: missing-policy"
    );
}

#[test]
fn compile_snapshot_skips_models_with_unknown_policy() {
    let mut broken_model = model();
    broken_model.policy_id = Some("missing-policy".to_string());

    let report = compile_snapshot(
        vec![provider()],
        vec![broken_model],
        vec![],
        vec![policy()],
        1,
    )
    .expect("unknown model policy should be skipped");

    assert!(report.snapshot.models_by_name.is_empty());
    assert!(report.snapshot.model_limits.is_empty());
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, "model");
    assert_eq!(report.issues[0].id, "gpt-4o-mini");
    assert_eq!(
        report.issues[0].reason,
        "missing policy reference: missing-policy"
    );
}

#[test]
fn compile_snapshot_skips_providers_with_unknown_policy() {
    let mut broken_provider = provider();
    broken_provider.policy_id = Some("missing-policy".to_string());

    let report = compile_snapshot(
        vec![broken_provider],
        vec![model()],
        vec![],
        vec![policy()],
        1,
    )
    .expect("unknown provider policy should be skipped");

    assert!(report.snapshot.providers_by_id.is_empty());
    assert!(report.snapshot.models_by_name.is_empty());
    assert_eq!(report.issues.len(), 2);
    assert_eq!(report.issues[0].kind, "provider");
    assert_eq!(report.issues[0].id, "provider-1");
    assert_eq!(
        report.issues[0].reason,
        "missing policy reference: missing-policy"
    );
    assert_eq!(report.issues[1].kind, "model");
    assert_eq!(report.issues[1].id, "gpt-4o-mini");
    assert_eq!(
        report.issues[1].reason,
        "missing provider reference: provider-1"
    );
}

#[test]
fn compile_snapshot_cascades_invalidation_without_retaining_old_dependents() {
    let mut broken_provider = provider();
    broken_provider.policy_id = Some("missing-policy".to_string());

    let mut broken_model = model();
    broken_model.id = "broken-model".to_string();

    let mut broken_key = api_key();
    broken_key.id = "broken-key".to_string();
    broken_key.key = "sk-broken".to_string();
    broken_key.allowed_models = vec!["broken-model".to_string()];

    let report = compile_snapshot(
        vec![broken_provider],
        vec![broken_model],
        vec![broken_key],
        vec![policy()],
        3,
    )
    .expect("invalid dependency chain should be reported and skipped");

    assert!(report.snapshot.providers_by_id.is_empty());
    assert!(report.snapshot.models_by_name.is_empty());
    assert!(report.snapshot.apikeys_by_id.is_empty());
    assert!(report.snapshot.keys_by_token.is_empty());
    assert_eq!(report.issues.len(), 3);
    assert_eq!(report.issues[0].kind, "provider");
    assert_eq!(report.issues[0].id, "provider-1");
    assert_eq!(
        report.issues[0].reason,
        "missing policy reference: missing-policy"
    );
    assert_eq!(report.issues[1].kind, "model");
    assert_eq!(report.issues[1].id, "broken-model");
    assert_eq!(
        report.issues[1].reason,
        "missing provider reference: provider-1"
    );
    assert_eq!(report.issues[2].kind, "api key");
    assert_eq!(report.issues[2].id, "broken-key");
    assert_eq!(
        report.issues[2].reason,
        "missing model reference: broken-model"
    );
}

#[test]
fn compile_snapshot_aggregates_multiple_independent_issues_while_keeping_valid_resources() {
    let mut broken_provider = provider();
    broken_provider.id = "provider-broken".to_string();
    broken_provider.policy_id = Some("missing-policy".to_string());

    let mut broken_model = model();
    broken_model.id = "broken-model".to_string();
    broken_model.provider_id = "missing-provider".to_string();

    let mut broken_key = api_key();
    broken_key.id = "broken-key".to_string();
    broken_key.key = "sk-broken".to_string();
    broken_key.allowed_models = vec!["missing-model".to_string()];

    let report = compile_snapshot(
        vec![provider(), broken_provider],
        vec![model(), broken_model],
        vec![api_key(), broken_key],
        vec![policy()],
        8,
    )
    .expect("independent invalid resources should be aggregated and skipped");

    assert!(report.snapshot.providers_by_id.contains_key("provider-1"));
    assert!(report.snapshot.models_by_name.contains_key("gpt-4o-mini"));
    assert!(report.snapshot.keys_by_token.contains_key("sk-test"));
    assert!(!report
        .snapshot
        .providers_by_id
        .contains_key("provider-broken"));
    assert!(!report.snapshot.models_by_name.contains_key("broken-model"));
    assert!(!report.snapshot.keys_by_token.contains_key("sk-broken"));
    assert_eq!(report.issues.len(), 3);
    assert_eq!(report.issues[0].kind, "provider");
    assert_eq!(report.issues[0].id, "provider-broken");
    assert_eq!(
        report.issues[0].reason,
        "missing policy reference: missing-policy"
    );
    assert_eq!(report.issues[1].kind, "model");
    assert_eq!(report.issues[1].id, "broken-model");
    assert_eq!(
        report.issues[1].reason,
        "missing provider reference: missing-provider"
    );
    assert_eq!(report.issues[2].kind, "api key");
    assert_eq!(report.issues[2].id, "broken-key");
    assert_eq!(
        report.issues[2].reason,
        "missing model reference: missing-model"
    );
}

#[test]
fn compile_snapshot_does_not_keep_cache_modes_for_skipped_provider_or_model() {
    let mut broken_provider = provider();
    broken_provider.id = "provider-broken".to_string();
    broken_provider.policy_id = Some("missing-policy".to_string());
    broken_provider.cache = Some(CachePolicyConfig {
        mode: CacheMode::Disabled,
    });

    let mut broken_model = model();
    broken_model.id = "model-broken".to_string();
    broken_model.provider_id = "provider-broken".to_string();
    broken_model.cache = Some(CachePolicyConfig {
        mode: CacheMode::Enabled,
    });

    let report = compile_snapshot(
        vec![provider(), broken_provider],
        vec![model(), broken_model],
        vec![],
        vec![policy()],
        200,
    )
    .expect("invalid provider/model should be skipped");

    let snapshot = report.snapshot;
    assert_eq!(snapshot.provider_cache_modes.len(), 1);
    assert_eq!(snapshot.model_cache_modes.len(), 1);
    assert_eq!(
        snapshot.provider_cache_modes.get("provider-1"),
        Some(&CacheMode::Inherit)
    );
    assert_eq!(
        snapshot.model_cache_modes.get("gpt-4o-mini"),
        Some(&CacheMode::Inherit)
    );
    assert!(!snapshot
        .provider_cache_modes
        .contains_key("provider-broken"));
    assert!(!snapshot.model_cache_modes.contains_key("model-broken"));
}

use std::collections::HashMap;

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig};
use crate::snapshot::{CompiledSnapshot, ResolvedLimits};

pub fn compile_snapshot(
    providers: Vec<ProviderConfig>,
    models: Vec<ModelConfig>,
    apikeys: Vec<ApiKeyConfig>,
    policies: Vec<PolicyConfig>,
    revision: i64,
) -> Result<CompiledSnapshot, String> {
    let policies_by_id = collect_unique_by_id(policies, "policy")?;

    let mut provider_limits = HashMap::new();
    for provider in &providers {
        validate_policy_reference(provider.policy_id.as_deref(), &policies_by_id)?;
        provider_limits.insert(
            provider.id.clone(),
            resolve_limits(
                provider.rate_limit.as_ref(),
                provider.policy_id.as_deref(),
                &policies_by_id,
            )?,
        );
    }

    let providers_by_id = collect_unique_by_id(providers, "provider")?;

    let mut model_limits = HashMap::new();
    for model in &models {
        if !providers_by_id.contains_key(&model.provider_id) {
            return Err(format!("missing provider reference: {}", model.provider_id));
        }
        validate_policy_reference(model.policy_id.as_deref(), &policies_by_id)?;
        model_limits.insert(
            model.id.clone(),
            resolve_limits(
                model.rate_limit.as_ref(),
                model.policy_id.as_deref(),
                &policies_by_id,
            )?,
        );
    }

    let models_by_name = collect_unique_by_id(models, "model")?;

    let apikeys_by_id = collect_unique_by_id(apikeys, "api key")?;
    let mut keys_by_token = HashMap::new();
    let mut key_limits = HashMap::new();

    for api_key in apikeys_by_id.values() {
        for model_name in &api_key.allowed_models {
            if !models_by_name.contains_key(model_name) {
                return Err(format!("missing model reference: {model_name}"));
            }
        }
        validate_policy_reference(api_key.policy_id.as_deref(), &policies_by_id)?;

        let resolved_limits = resolve_limits(
            api_key.rate_limit.as_ref(),
            api_key.policy_id.as_deref(),
            &policies_by_id,
        )?;

        let token = api_key.key.clone();
        if keys_by_token.contains_key(&token) {
            return Err("duplicate api key token".to_string());
        }

        key_limits.insert(api_key.id.clone(), resolved_limits);
        let previous = keys_by_token.insert(
            token.clone(),
            KeyMeta {
                key_id: api_key.id.clone(),
                user_id: None,
                customer_id: None,
                alias: None,
                expires_at: None,
                allowed_models: api_key.allowed_models.clone(),
            },
        );
        debug_assert!(previous.is_none());
    }

    Ok(CompiledSnapshot {
        revision,
        keys_by_token,
        apikeys_by_id,
        providers_by_id,
        models_by_name,
        policies_by_id,
        provider_limits,
        model_limits,
        key_limits,
    })
}

fn resolve_limits(
    inline_limits: Option<&crate::etcd_model::RateLimitConfig>,
    policy_id: Option<&str>,
    policies_by_id: &HashMap<String, PolicyConfig>,
) -> Result<ResolvedLimits, String> {
    let mut resolved = match policy_id {
        Some(policy_id) => {
            let policy = policies_by_id
                .get(policy_id)
                .ok_or_else(|| format!("missing policy: {policy_id}"))?;
            ResolvedLimits::from(&policy.rate_limit)
        }
        None => ResolvedLimits {
            rpm: None,
            tpm: None,
            concurrency: None,
        },
    };

    if let Some(inline_limits) = inline_limits {
        if inline_limits.rpm.is_some() {
            resolved.rpm = inline_limits.rpm;
        }
        if inline_limits.tpm.is_some() {
            resolved.tpm = inline_limits.tpm;
        }
        if inline_limits.concurrency.is_some() {
            resolved.concurrency = inline_limits.concurrency;
        }
    }

    Ok(resolved)
}

fn collect_unique_by_id<T>(items: Vec<T>, kind: &str) -> Result<HashMap<String, T>, String>
where
    T: HasConfigId,
{
    let mut collected = HashMap::new();

    for item in items {
        let id = item.config_id().to_string();
        if collected.insert(id.clone(), item).is_some() {
            return Err(format!("duplicate {kind} id: {id}"));
        }
    }

    Ok(collected)
}

trait HasConfigId {
    fn config_id(&self) -> &str;
}

impl HasConfigId for PolicyConfig {
    fn config_id(&self) -> &str {
        &self.id
    }
}

impl HasConfigId for ProviderConfig {
    fn config_id(&self) -> &str {
        &self.id
    }
}

impl HasConfigId for ModelConfig {
    fn config_id(&self) -> &str {
        &self.id
    }
}

impl HasConfigId for ApiKeyConfig {
    fn config_id(&self) -> &str {
        &self.id
    }
}

fn validate_policy_reference(
    policy_id: Option<&str>,
    policies_by_id: &HashMap<String, PolicyConfig>,
) -> Result<(), String> {
    if let Some(policy_id) = policy_id {
        if !policies_by_id.contains_key(policy_id) {
            return Err(format!("missing policy reference: {policy_id}"));
        }
    }

    Ok(())
}

use std::collections::HashMap;

use aisix_types::entities::KeyMeta;

use crate::etcd_model::{ApiKeyConfig, CacheMode, ModelConfig, PolicyConfig, ProviderConfig};
use crate::snapshot::{CompiledSnapshot, ResolvedLimits};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileIssue {
    pub kind: &'static str,
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCompileReport {
    pub snapshot: CompiledSnapshot,
    pub issues: Vec<CompileIssue>,
}

pub fn compile_snapshot(
    providers: Vec<ProviderConfig>,
    models: Vec<ModelConfig>,
    apikeys: Vec<ApiKeyConfig>,
    policies: Vec<PolicyConfig>,
    revision: i64,
) -> Result<SnapshotCompileReport, String> {
    let policies_by_id = collect_unique_by_id(policies, "policy")?;
    ensure_unique_ids(&providers, "provider")?;
    ensure_unique_ids(&models, "model")?;
    ensure_unique_ids(&apikeys, "api key")?;
    ensure_unique_api_key_tokens(&apikeys)?;

    let mut issues = Vec::new();
    let mut providers_by_id = HashMap::new();
    let mut provider_limits = HashMap::new();
    let mut provider_cache_modes = HashMap::new();
    for provider in providers {
        if let Some(reason) = missing_policy_reason(provider.policy_id.as_deref(), &policies_by_id)
        {
            issues.push(CompileIssue {
                kind: "provider",
                id: provider.id,
                reason,
            });
            continue;
        }

        provider_cache_modes.insert(
            provider.id.clone(),
            provider
                .cache
                .as_ref()
                .map(|cache| cache.mode)
                .unwrap_or(CacheMode::Inherit),
        );

        provider_limits.insert(
            provider.id.clone(),
            resolve_limits(
                provider.rate_limit.as_ref(),
                provider.policy_id.as_deref(),
                &policies_by_id,
            )?,
        );
        providers_by_id.insert(provider.id.clone(), provider);
    }

    let mut models_by_name = HashMap::new();
    let mut model_limits = HashMap::new();
    let mut model_cache_modes = HashMap::new();
    for model in models {
        if !providers_by_id.contains_key(&model.provider_id) {
            issues.push(CompileIssue {
                kind: "model",
                id: model.id,
                reason: format!("missing provider reference: {}", model.provider_id),
            });
            continue;
        }

        if let Some(reason) = missing_policy_reason(model.policy_id.as_deref(), &policies_by_id) {
            issues.push(CompileIssue {
                kind: "model",
                id: model.id,
                reason,
            });
            continue;
        }

        model_cache_modes.insert(
            model.id.clone(),
            model
                .cache
                .as_ref()
                .map(|cache| cache.mode)
                .unwrap_or(CacheMode::Inherit),
        );

        model_limits.insert(
            model.id.clone(),
            resolve_limits(
                model.rate_limit.as_ref(),
                model.policy_id.as_deref(),
                &policies_by_id,
            )?,
        );
        models_by_name.insert(model.id.clone(), model);
    }

    let mut apikeys_by_id = HashMap::new();
    let mut keys_by_token = HashMap::new();
    let mut key_limits = HashMap::new();

    for api_key in apikeys {
        if let Some(model_name) = api_key
            .allowed_models
            .iter()
            .find(|model_name| !models_by_name.contains_key(*model_name))
        {
            issues.push(CompileIssue {
                kind: "api key",
                id: api_key.id,
                reason: format!("missing model reference: {model_name}"),
            });
            continue;
        }

        if let Some(reason) = missing_policy_reason(api_key.policy_id.as_deref(), &policies_by_id) {
            issues.push(CompileIssue {
                kind: "api key",
                id: api_key.id,
                reason,
            });
            continue;
        }

        let resolved_limits = resolve_limits(
            api_key.rate_limit.as_ref(),
            api_key.policy_id.as_deref(),
            &policies_by_id,
        )?;

        let token = api_key.key.clone();
        key_limits.insert(api_key.id.clone(), resolved_limits);
        apikeys_by_id.insert(api_key.id.clone(), api_key.clone());
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

    Ok(SnapshotCompileReport {
        snapshot: CompiledSnapshot {
            revision,
            keys_by_token,
            apikeys_by_id,
            providers_by_id,
            models_by_name,
            policies_by_id,
            provider_limits,
            model_limits,
            key_limits,
            provider_cache_modes,
            model_cache_modes,
        },
        issues,
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
                .ok_or_else(|| format!("missing policy reference: {policy_id}"))?;
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

fn ensure_unique_ids<T>(items: &[T], kind: &str) -> Result<(), String>
where
    T: HasConfigId,
{
    let mut seen = HashMap::new();

    for item in items {
        let id = item.config_id().to_string();
        if seen.insert(id.clone(), ()).is_some() {
            return Err(format!("duplicate {kind} id: {id}"));
        }
    }

    Ok(())
}

fn ensure_unique_api_key_tokens(apikeys: &[ApiKeyConfig]) -> Result<(), String> {
    let mut seen = HashMap::new();

    for api_key in apikeys {
        if seen.insert(api_key.key.clone(), ()).is_some() {
            return Err("duplicate api key token".to_string());
        }
    }

    Ok(())
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

fn missing_policy_reason(
    policy_id: Option<&str>,
    policies_by_id: &HashMap<String, PolicyConfig>,
) -> Option<String> {
    if let Some(policy_id) = policy_id {
        if !policies_by_id.contains_key(policy_id) {
            return Some(format!("missing policy reference: {policy_id}"));
        }
    }

    None
}

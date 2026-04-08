use serde::Serialize;

use crate::{
    compile::compile_snapshot,
    etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig},
    snapshot::CompiledSnapshot,
};

trait HasConfigId {
    fn config_id(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcdEntry {
    pub key: String,
    pub value: Vec<u8>,
}

impl EtcdEntry {
    pub fn json(key: &str, value: &impl Serialize) -> Self {
        Self {
            key: key.to_string(),
            value: serde_json::to_vec(value).expect("json fixture should serialize"),
        }
    }
}

pub fn compile_snapshot_from_entries(
    prefix: &str,
    entries: &[EtcdEntry],
    revision: i64,
) -> Result<CompiledSnapshot, String> {
    let normalized_prefix = format!("{}/", prefix.trim_end_matches('/'));
    let mut providers = Vec::new();
    let mut models = Vec::new();
    let mut apikeys = Vec::new();
    let mut policies = Vec::new();

    for entry in entries {
        let relative = entry
            .key
            .strip_prefix(&normalized_prefix)
            .ok_or_else(|| format!("invalid etcd key outside prefix: {}", entry.key))?;

        let mut parts = relative.split('/');
        let collection = parts
            .next()
            .filter(|segment| !segment.is_empty())
            .ok_or_else(|| format!("invalid etcd key: {}", entry.key))?;
        let resource_id = parts
            .next()
            .filter(|segment| !segment.is_empty())
            .ok_or_else(|| format!("invalid etcd key: {}", entry.key))?;

        if parts.next().is_some() || resource_id.is_empty() {
            return Err(format!("invalid etcd key: {}", entry.key));
        }

        match collection {
            "providers" => providers.push(decode_entry::<ProviderConfig>(
                entry,
                "provider",
                resource_id,
            )?),
            "models" => models.push(decode_entry::<ModelConfig>(entry, "model", resource_id)?),
            "apikeys" => apikeys.push(decode_entry::<ApiKeyConfig>(entry, "api key", resource_id)?),
            "policies" => {
                policies.push(decode_entry::<PolicyConfig>(entry, "policy", resource_id)?)
            }
            other => return Err(format!("unsupported etcd collection: {other}")),
        }
    }

    compile_snapshot(providers, models, apikeys, policies, revision)
}

fn decode_entry<T>(entry: &EtcdEntry, kind: &str, resource_id: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + HasConfigId,
{
    let decoded = serde_json::from_slice::<T>(&entry.value)
        .map_err(|error| format!("failed to decode {kind} at {}: {error}", entry.key))?;

    if decoded.config_id() != resource_id {
        return Err(format!(
            "etcd key/body id mismatch for {kind} at {}: expected {resource_id}, got {}",
            entry.key,
            decoded.config_id()
        ));
    }

    Ok(decoded)
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

impl HasConfigId for PolicyConfig {
    fn config_id(&self) -> &str {
        &self.id
    }
}

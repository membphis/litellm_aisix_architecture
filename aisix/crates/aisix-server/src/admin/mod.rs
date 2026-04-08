pub mod apikeys;
pub mod auth;
pub mod models;
pub mod policies;
pub mod providers;

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

use aisix_config::{
    compile::compile_snapshot,
    etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig},
    snapshot::CompiledSnapshot,
    startup::StartupConfig,
};
use aisix_types::error::{ErrorKind, GatewayError};
use arc_swap::ArcSwap;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AdminState {
    keys: Arc<HashSet<String>>,
    store: AdminStore,
}

#[derive(Debug, Clone)]
struct AdminStore {
    prefix: String,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    inner: Arc<Mutex<StoreInner>>,
}

#[derive(Debug, Default)]
struct StoreInner {
    revision: i64,
    entries: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminWriteResult {
    pub id: String,
    pub path: String,
    pub revision: i64,
}

impl AdminState {
    pub fn new(
        prefix: String,
        admin_keys: Vec<String>,
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    ) -> Self {
        let seeded = StoreInner::from_snapshot(prefix.trim_end_matches('/'), &snapshot.load());
        Self {
            keys: Arc::new(admin_keys.into_iter().collect()),
            store: AdminStore {
                prefix,
                snapshot,
                inner: Arc::new(Mutex::new(seeded)),
            },
        }
    }

    pub fn from_startup_config(
        config: &StartupConfig,
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    ) -> Option<Self> {
        if !config.deployment.admin.enabled {
            return None;
        }

        Some(Self::new(
            config.etcd.prefix.clone(),
            config
                .deployment
                .admin
                .admin_keys
                .iter()
                .map(|key| key.key.clone())
                .collect(),
            snapshot,
        ))
    }

    pub fn is_authorized(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    pub fn put_provider(
        &self,
        id: &str,
        provider: ProviderConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.store.put("providers", id, &provider)
    }

    pub fn put_model(
        &self,
        id: &str,
        model: ModelConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.store.put("models", id, &model)
    }

    pub fn put_apikey(
        &self,
        id: &str,
        apikey: ApiKeyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.store.put("apikeys", id, &apikey)
    }

    pub fn put_policy(
        &self,
        id: &str,
        policy: PolicyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.store.put("policies", id, &policy)
    }
}

impl StoreInner {
    fn from_snapshot(prefix: &str, snapshot: &CompiledSnapshot) -> Self {
        let mut entries = BTreeMap::new();

        for provider in snapshot.providers_by_id.values() {
            let path = format!("{prefix}/providers/{}", provider.id);
            entries.insert(path, serde_json::to_vec(provider).expect("provider config should serialize"));
        }

        for model in snapshot.models_by_name.values() {
            let path = format!("{prefix}/models/{}", model.id);
            entries.insert(path, serde_json::to_vec(model).expect("model config should serialize"));
        }

        for policy in snapshot.policies_by_id.values() {
            let path = format!("{prefix}/policies/{}", policy.id);
            entries.insert(path, serde_json::to_vec(policy).expect("policy config should serialize"));
        }

        for config in snapshot.apikeys_by_id.values() {
            let path = format!("{prefix}/apikeys/{}", config.id);
            entries.insert(path, serde_json::to_vec(config).expect("api key config should serialize"));
        }

        Self {
            revision: snapshot.revision,
            entries,
        }
    }
}

impl AdminStore {
    fn put<T>(
        &self,
        collection: &str,
        id: &str,
        value: &T,
    ) -> Result<AdminWriteResult, GatewayError>
    where
        T: Serialize,
    {
        let path = self.path(collection, id);
        let bytes = serde_json::to_vec(value).map_err(invalid_request)?;
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut staged_entries = inner.entries.clone();
        staged_entries.insert(path.clone(), bytes);

        let revision = inner.revision + 1;
        let snapshot = compile_from_entries(&self.prefix, &staged_entries, revision)?;
        self.snapshot.store(Arc::new(snapshot));

        inner.entries = staged_entries;
        inner.revision = revision;

        Ok(AdminWriteResult {
            id: id.to_string(),
            path,
            revision,
        })
    }

    fn path(&self, collection: &str, id: &str) -> String {
        format!("{}/{collection}/{id}", self.prefix.trim_end_matches('/'))
    }
}

fn compile_from_entries(
    prefix: &str,
    entries: &BTreeMap<String, Vec<u8>>,
    revision: i64,
) -> Result<CompiledSnapshot, GatewayError> {
    let normalized_prefix = format!("{}/", prefix.trim_end_matches('/'));
    let mut providers = Vec::new();
    let mut models = Vec::new();
    let mut apikeys = Vec::new();
    let mut policies = Vec::new();

    for (path, value) in entries {
        let relative = path
            .strip_prefix(&normalized_prefix)
            .ok_or_else(|| GatewayError {
                kind: ErrorKind::Internal,
                message: format!("invalid admin store path: {path}"),
            })?;
        let (collection, _) = relative.split_once('/').ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: format!("invalid admin store path: {path}"),
        })?;

        match collection {
            "providers" => providers.push(serde_json::from_slice(value).map_err(invalid_request)?),
            "models" => models.push(serde_json::from_slice(value).map_err(invalid_request)?),
            "apikeys" => apikeys.push(serde_json::from_slice(value).map_err(invalid_request)?),
            "policies" => policies.push(serde_json::from_slice(value).map_err(invalid_request)?),
            other => {
                return Err(GatewayError {
                    kind: ErrorKind::Internal,
                    message: format!("unsupported admin store collection: {other}"),
                });
            }
        }
    }

    compile_snapshot(providers, models, apikeys, policies, revision).map_err(|message| {
        GatewayError {
            kind: ErrorKind::InvalidRequest,
            message,
        }
    })
}

fn invalid_request(error: impl std::fmt::Display) -> GatewayError {
    GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: error.to_string(),
    }
}

pub fn ensure_path_matches_body_id(path_id: &str, body_id: &str) -> Result<(), GatewayError> {
    if path_id == body_id {
        return Ok(());
    }

    Err(GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: format!("path id '{path_id}' does not match body id '{body_id}'"),
    })
}

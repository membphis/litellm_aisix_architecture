pub mod apikeys;
pub mod auth;
pub mod models;
pub mod policies;
pub mod providers;

use std::{collections::HashSet, sync::Arc};

use aisix_config::{
    etcd::EtcdStore,
    etcd_model::{ApiKeyConfig, ModelConfig, PolicyConfig, ProviderConfig},
    startup::StartupConfig,
};
use aisix_types::error::{ErrorKind, GatewayError};
use serde::Serialize;

#[derive(Clone)]
pub struct AdminState {
    keys: Arc<HashSet<String>>,
    prefix: String,
    etcd: Arc<tokio::sync::Mutex<EtcdStore>>,
}

impl std::fmt::Debug for AdminState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminState")
            .field("keys", &self.keys)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminWriteResult {
    pub id: String,
    pub path: String,
    pub revision: i64,
}

impl AdminState {
    pub async fn from_startup_config(config: &StartupConfig) -> anyhow::Result<Option<Self>> {
        if !config.deployment.admin.enabled {
            return Ok(None);
        }

        let etcd = EtcdStore::connect(&config.etcd).await?;
        Ok(Some(Self {
            keys: Arc::new(
                config
                    .deployment
                    .admin
                    .admin_keys
                    .iter()
                    .map(|key| key.key.clone())
                    .collect(),
            ),
            prefix: config.etcd.prefix.clone(),
            etcd: Arc::new(tokio::sync::Mutex::new(etcd)),
        }))
    }

    pub fn is_authorized(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    pub async fn put_provider(
        &self,
        id: &str,
        provider: ProviderConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("providers", id, &provider).await
    }

    pub async fn put_model(
        &self,
        id: &str,
        model: ModelConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("models", id, &model).await
    }

    pub async fn put_apikey(
        &self,
        id: &str,
        apikey: ApiKeyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("apikeys", id, &apikey).await
    }

    pub async fn put_policy(
        &self,
        id: &str,
        policy: PolicyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("policies", id, &policy).await
    }

    async fn put<T>(
        &self,
        collection: &str,
        id: &str,
        value: &T,
    ) -> Result<AdminWriteResult, GatewayError>
    where
        T: Serialize,
    {
        let mut etcd = self.etcd.lock().await;
        let write = etcd
            .put_json(&self.prefix, collection, id, value)
            .await
            .map_err(internal_admin_error)?;

        Ok(AdminWriteResult {
            id: id.to_string(),
            path: write.key,
            revision: write.revision,
        })
    }
}

fn internal_admin_error(error: impl std::fmt::Display) -> GatewayError {
    GatewayError {
        kind: ErrorKind::Internal,
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

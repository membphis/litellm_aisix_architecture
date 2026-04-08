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
            .field("key_count", &self.keys.len())
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

    pub async fn get_provider(&self, id: &str) -> Result<ProviderConfig, GatewayError> {
        self.get("providers", id).await
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderConfig>, GatewayError> {
        let mut providers = self.list("providers").await?;
        providers.sort_by(|left: &ProviderConfig, right: &ProviderConfig| left.id.cmp(&right.id));
        Ok(providers)
    }

    pub async fn delete_provider(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
        self.delete("providers", id).await
    }

    pub async fn get_model(&self, id: &str) -> Result<ModelConfig, GatewayError> {
        self.get("models", id).await
    }

    pub async fn list_models(&self) -> Result<Vec<ModelConfig>, GatewayError> {
        let mut models = self.list("models").await?;
        models.sort_by(|left: &ModelConfig, right: &ModelConfig| left.id.cmp(&right.id));
        Ok(models)
    }

    pub async fn delete_model(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
        self.delete("models", id).await
    }

    pub async fn put_apikey(
        &self,
        id: &str,
        apikey: ApiKeyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("apikeys", id, &apikey).await
    }

    pub async fn get_apikey(&self, id: &str) -> Result<ApiKeyConfig, GatewayError> {
        self.get("apikeys", id).await
    }

    pub async fn list_apikeys(&self) -> Result<Vec<ApiKeyConfig>, GatewayError> {
        let mut apikeys = self.list("apikeys").await?;
        apikeys.sort_by(|left: &ApiKeyConfig, right: &ApiKeyConfig| left.id.cmp(&right.id));
        Ok(apikeys)
    }

    pub async fn delete_apikey(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
        self.delete("apikeys", id).await
    }

    pub async fn put_policy(
        &self,
        id: &str,
        policy: PolicyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.put("policies", id, &policy).await
    }

    pub async fn get_policy(&self, id: &str) -> Result<PolicyConfig, GatewayError> {
        self.get("policies", id).await
    }

    pub async fn list_policies(&self) -> Result<Vec<PolicyConfig>, GatewayError> {
        let mut policies = self.list("policies").await?;
        policies.sort_by(|left: &PolicyConfig, right: &PolicyConfig| left.id.cmp(&right.id));
        Ok(policies)
    }

    pub async fn delete_policy(&self, id: &str) -> Result<AdminWriteResult, GatewayError> {
        self.delete("policies", id).await
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

    async fn get<T>(&self, collection: &str, id: &str) -> Result<T, GatewayError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut etcd = self.etcd.lock().await;
        etcd.get_json(&self.prefix, collection, id)
            .await
            .map_err(internal_admin_error)?
            .ok_or_else(|| missing_admin_resource(collection, id))
    }

    async fn list<T>(&self, collection: &str) -> Result<Vec<T>, GatewayError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut etcd = self.etcd.lock().await;
        etcd.list_json(&self.prefix, collection)
            .await
            .map_err(internal_admin_error)
    }

    async fn delete(&self, collection: &str, id: &str) -> Result<AdminWriteResult, GatewayError> {
        let mut etcd = self.etcd.lock().await;
        let deleted = etcd
            .delete(&self.prefix, collection, id)
            .await
            .map_err(internal_admin_error)?;

        if !deleted.existed {
            return Err(missing_admin_resource(collection, id));
        }

        Ok(AdminWriteResult {
            id: id.to_string(),
            path: deleted.key,
            revision: deleted.revision,
        })
    }
}

fn internal_admin_error(error: impl std::fmt::Display) -> GatewayError {
    GatewayError {
        kind: ErrorKind::Internal,
        message: error.to_string(),
    }
}

fn missing_admin_resource(collection: &str, id: &str) -> GatewayError {
    GatewayError {
        kind: ErrorKind::NotFound,
        message: format!("{collection} '{id}' not found"),
    }
}

pub fn ensure_path_matches_body_id(path_id: &str, body_id: &str) -> Result<(), GatewayError> {
    ensure_valid_resource_id(path_id)?;
    ensure_valid_resource_id(body_id)?;

    if path_id == body_id {
        return Ok(());
    }

    Err(GatewayError {
        kind: ErrorKind::InvalidRequest,
        message: format!("path id '{path_id}' does not match body id '{body_id}'"),
    })
}

pub fn ensure_valid_resource_id(id: &str) -> Result<(), GatewayError> {
    if id.contains('/') {
        return Err(GatewayError {
            kind: ErrorKind::InvalidRequest,
            message: format!("admin resource id '{id}' must not contain '/'"),
        });
    }

    Ok(())
}

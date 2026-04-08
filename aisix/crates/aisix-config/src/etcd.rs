use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{loader::EtcdEntry, startup::EtcdConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminStoreWrite {
    pub key: String,
    pub revision: i64,
}

pub fn resource_key(prefix: &str, collection: &str, id: &str) -> String {
    format!("{}/{collection}/{id}", prefix.trim_end_matches('/'))
}

pub struct EtcdStore {
    client: etcd_client::Client,
}

impl EtcdStore {
    pub async fn connect(config: &EtcdConfig) -> Result<Self> {
        let options = etcd_client::ConnectOptions::default()
            .with_connect_timeout(Duration::from_millis(config.dial_timeout_ms));
        let client = etcd_client::Client::connect(config.endpoints.clone(), Some(options))
            .await
            .context("failed to connect to etcd")?;

        Ok(Self { client })
    }

    pub async fn load_prefix(&mut self, prefix: &str) -> Result<(Vec<EtcdEntry>, i64)> {
        let response = self
            .client
            .get(
                prefix.trim_end_matches('/'),
                Some(etcd_client::GetOptions::new().with_prefix()),
            )
            .await
            .context("failed to load config from etcd")?;

        let revision = response.header().map(|header| header.revision()).unwrap_or(0);
        let entries = response
            .kvs()
            .iter()
            .map(|kv| EtcdEntry {
                key: String::from_utf8_lossy(kv.key()).into_owned(),
                value: kv.value().to_vec(),
            })
            .collect();

        Ok((entries, revision))
    }

    pub async fn put_json<T: Serialize>(
        &mut self,
        prefix: &str,
        collection: &str,
        id: &str,
        value: &T,
    ) -> Result<AdminStoreWrite> {
        let key = resource_key(prefix, collection, id);
        let body = serde_json::to_vec(value).context("failed to serialize admin payload")?;
        let response = self
            .client
            .put(key.clone(), body, None)
            .await
            .context("failed to write admin config")?;

        Ok(AdminStoreWrite {
            key,
            revision: response.header().map(|header| header.revision()).unwrap_or(0),
        })
    }

    pub async fn delete(
        &mut self,
        prefix: &str,
        collection: &str,
        id: &str,
    ) -> Result<AdminStoreWrite> {
        let key = resource_key(prefix, collection, id);
        let response = self
            .client
            .delete(key.clone(), None)
            .await
            .context("failed to delete admin config")?;

        Ok(AdminStoreWrite {
            key,
            revision: response.header().map(|header| header.revision()).unwrap_or(0),
        })
    }

    pub async fn watch_prefix(
        &mut self,
        prefix: &str,
        start_revision: Option<i64>,
    ) -> Result<etcd_client::WatchStream> {
        let mut options = etcd_client::WatchOptions::new().with_prefix();
        if let Some(revision) = start_revision {
            options = options.with_start_revision(revision);
        }

        let (_, stream) = self
            .client
            .watch(prefix.trim_end_matches('/'), Some(options))
            .await
            .context("failed to watch config prefix in etcd")?;

        Ok(stream)
    }
}

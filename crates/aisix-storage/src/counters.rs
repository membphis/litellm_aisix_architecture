use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::redis_pool::{RedisError, RedisPool};

#[derive(Debug, Clone, Default)]
pub struct CounterStore {
    redis: Option<RedisPool>,
    local_totals: Arc<Mutex<HashMap<String, u64>>>,
}

impl CounterStore {
    pub fn new(redis: Option<RedisPool>) -> Self {
        Self {
            redis,
            local_totals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn incr_minute_window(
        &self,
        prefix: &str,
        minute_bucket: u64,
        ttl_seconds: u64,
    ) -> Result<u64, RedisError> {
        let redis = self
            .redis
            .as_ref()
            .ok_or_else(|| RedisError::Unavailable("redis pool not configured".to_string()))?;
        let key = format!("{prefix}:{minute_bucket}");
        redis.incr(&key, ttl_seconds).await
    }

    pub async fn incr_total(&self, key: &str, amount: u64) -> u64 {
        let local_value = {
            let mut totals = self
                .local_totals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = totals.entry(key.to_string()).or_insert(0);
            *entry += amount;
            *entry
        };

        if let Some(redis) = &self.redis {
            let _ = redis.incr_by(key, amount).await;
        }

        local_value
    }

    pub fn total_for(&self, key: &str) -> u64 {
        let totals = self
            .local_totals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        totals.get(key).copied().unwrap_or(0)
    }
}

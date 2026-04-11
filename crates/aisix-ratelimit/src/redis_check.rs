use aisix_storage::CounterStore;

use crate::shadow::current_minute_bucket;

#[derive(Debug, Clone)]
pub struct RedisCheck {
    counters: CounterStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedisCheckResult {
    Allowed,
    Limited,
    Unavailable,
}

impl RedisCheck {
    pub fn new(counters: CounterStore) -> Self {
        Self { counters }
    }

    pub async fn check_rpm(&self, key: &str, limit: u64) -> RedisCheckResult {
        match self
            .counters
            .incr_minute_window(key, current_minute_bucket(), 90)
            .await
        {
            Ok(count) if count > limit => RedisCheckResult::Limited,
            Ok(_) => RedisCheckResult::Allowed,
            Err(_) => RedisCheckResult::Unavailable,
        }
    }
}

use aisix_storage::{CounterStore, RedisPool};
use aisix_types::{entities::KeyMeta, usage::Usage};

use crate::event::UsageEvent;

#[derive(Debug, Clone, Default)]
pub struct UsageRecorder {
    counters: CounterStore,
}

impl UsageRecorder {
    pub fn new(redis: Option<RedisPool>) -> Self {
        Self {
            counters: CounterStore::new(redis),
        }
    }

    pub async fn record_success(&self, key: &KeyMeta, model: &str, usage: &Usage) {
        let event = UsageEvent {
            key_id: key.key_id.clone(),
            model: model.to_string(),
            usage: usage.clone(),
        };

        let input_key = format!("usage:key:{}:input_tokens", event.key_id);
        let output_key = format!("usage:key:{}:output_tokens", event.key_id);
        let _ = self.counters.incr_total(&input_key, event.usage.input_tokens).await;
        let _ = self.counters.incr_total(&output_key, event.usage.output_tokens).await;
    }

    pub fn total_for(&self, key: &str) -> u64 {
        self.counters.total_for(key)
    }
}

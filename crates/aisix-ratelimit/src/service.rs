use aisix_config::snapshot::{CompiledSnapshot, ResolvedLimits};
use aisix_storage::{CounterStore, RedisPool};

use crate::{
    concurrency::{ConcurrencyGuard, ConcurrencyLimiter},
    redis_check::{RedisCheck, RedisCheckResult},
    shadow::{rate_limited, ShadowLimiter},
};

#[derive(Debug, Clone)]
pub struct RateLimitService {
    shadow: ShadowLimiter,
    redis: RedisCheck,
    concurrency: ConcurrencyLimiter,
}

impl RateLimitService {
    pub fn new(redis: Option<RedisPool>) -> Self {
        let counters = CounterStore::new(redis);
        Self {
            shadow: ShadowLimiter::default(),
            redis: RedisCheck::new(counters),
            concurrency: ConcurrencyLimiter::default(),
        }
    }

    pub async fn precheck(
        &self,
        snapshot: &CompiledSnapshot,
        key_id: &str,
        model_name: &str,
        provider_id: &str,
    ) -> Result<ConcurrencyGuard, aisix_types::error::GatewayError> {
        let limits = resolve_limits(snapshot, key_id, model_name, provider_id);
        let guard = self
            .concurrency
            .acquire(key_id.to_string(), limits.concurrency)?;

        if let Some(rpm) = limits.rpm {
            let redis_key = format!("ratelimit:rpm:key:{key_id}");
            match self.redis.check_rpm(&redis_key, rpm).await {
                RedisCheckResult::Allowed => {}
                RedisCheckResult::Limited => return Err(rate_limited()),
                RedisCheckResult::Unavailable => {
                    self.shadow.check_rpm(&redis_key, rpm)?;
                }
            }
        }

        Ok(guard)
    }
}

impl Default for RateLimitService {
    fn default() -> Self {
        Self::new(None)
    }
}

fn resolve_limits(
    snapshot: &CompiledSnapshot,
    key_id: &str,
    model_name: &str,
    provider_id: &str,
) -> ResolvedLimits {
    let key_limits = snapshot.key_limits.get(key_id);
    let model_limits = snapshot.model_limits.get(model_name);
    let provider_limits = snapshot.provider_limits.get(provider_id);

    ResolvedLimits {
        rpm: key_limits
            .and_then(|limits| limits.rpm)
            .or_else(|| model_limits.and_then(|limits| limits.rpm))
            .or_else(|| provider_limits.and_then(|limits| limits.rpm)),
        tpm: key_limits
            .and_then(|limits| limits.tpm)
            .or_else(|| model_limits.and_then(|limits| limits.tpm))
            .or_else(|| provider_limits.and_then(|limits| limits.tpm)),
        concurrency: key_limits
            .and_then(|limits| limits.concurrency)
            .or_else(|| model_limits.and_then(|limits| limits.concurrency))
            .or_else(|| provider_limits.and_then(|limits| limits.concurrency)),
    }
}

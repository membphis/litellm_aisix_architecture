use std::sync::Arc;

use aisix_cache::MemoryCache;
use aisix_config::snapshot::CompiledSnapshot;
use aisix_ratelimit::RateLimitService;
use aisix_spend::UsageRecorder;
use aisix_storage::RedisPool;
use arc_swap::ArcSwap;

#[derive(Debug, Clone)]
pub struct AppState {
    pub snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    pub ready: bool,
    pub cache: MemoryCache,
    pub rate_limits: RateLimitService,
    pub usage_recorder: UsageRecorder,
}

impl AppState {
    pub fn new(snapshot: Arc<ArcSwap<CompiledSnapshot>>, ready: bool) -> Self {
        Self::with_redis(snapshot, ready, None)
    }

    pub fn with_redis(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        redis: Option<RedisPool>,
    ) -> Self {
        Self {
            snapshot,
            ready,
            cache: MemoryCache::default(),
            rate_limits: RateLimitService::new(redis.clone()),
            usage_recorder: UsageRecorder::new(redis),
        }
    }
}

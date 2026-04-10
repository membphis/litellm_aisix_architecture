use std::sync::Arc;

use aisix_cache::MemoryCache;
use aisix_config::snapshot::CompiledSnapshot;
use aisix_config::watcher::SnapshotWatcher;
use aisix_ratelimit::RateLimitService;
use aisix_spend::UsageRecorder;
use aisix_storage::RedisPool;
use arc_swap::ArcSwap;

#[derive(Debug, Clone)]
pub struct AppState {
    pub snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    pub ready: bool,
    _watcher: Option<SnapshotWatcher>,
    pub cache: MemoryCache,
    pub default_cache_enabled: bool,
    pub rate_limits: RateLimitService,
    pub usage_recorder: UsageRecorder,
}

impl AppState {
    pub fn new(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
    ) -> Self {
        Self::with_redis_and_watcher(snapshot, ready, default_cache_enabled, None, None)
    }

    pub fn with_redis(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
        redis: Option<RedisPool>,
    ) -> Self {
        Self::with_redis_and_watcher(snapshot, ready, default_cache_enabled, redis, None)
    }

    pub fn with_redis_and_watcher(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
        redis: Option<RedisPool>,
        watcher: Option<SnapshotWatcher>,
    ) -> Self {
        Self {
            snapshot,
            ready,
            _watcher: watcher,
            cache: MemoryCache::default(),
            default_cache_enabled,
            rate_limits: RateLimitService::new(redis.clone()),
            usage_recorder: UsageRecorder::new(redis),
        }
    }
}

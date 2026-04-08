use anyhow::Result;
use aisix_config::{snapshot::CompiledSnapshot, startup::StartupConfig, watcher::initial_snapshot_handle};
use aisix_core::AppState;
use aisix_storage::RedisPool;

pub async fn bootstrap(config: &StartupConfig) -> Result<AppState> {
    let snapshot = empty_snapshot();
    let snapshot = initial_snapshot_handle(snapshot);
    let redis = RedisPool::from_url(&config.redis.url)?;

    Ok(AppState::with_redis(snapshot, true, Some(redis)))
}

fn empty_snapshot() -> CompiledSnapshot {
    CompiledSnapshot {
        revision: 0,
        keys_by_token: Default::default(),
        apikeys_by_id: Default::default(),
        providers_by_id: Default::default(),
        models_by_name: Default::default(),
        policies_by_id: Default::default(),
        provider_limits: Default::default(),
        model_limits: Default::default(),
        key_limits: Default::default(),
    }
}

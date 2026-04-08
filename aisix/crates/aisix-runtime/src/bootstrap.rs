use anyhow::Result;
use aisix_config::{
    startup::StartupConfig,
    watcher::{initial_snapshot_handle, load_initial_snapshot, spawn_snapshot_watcher},
};
use aisix_core::AppState;
use aisix_storage::RedisPool;

pub async fn bootstrap(config: &StartupConfig) -> Result<AppState> {
    let snapshot = load_initial_snapshot(config).await?;
    let snapshot = initial_snapshot_handle(snapshot);
    let redis = RedisPool::from_url(&config.redis.url)?;
    let watcher = spawn_snapshot_watcher(config.etcd.clone(), snapshot.clone()).await?;

    Ok(AppState::with_redis_and_watcher(
        snapshot.clone(),
        true,
        Some(redis),
        Some(watcher),
    ))
}

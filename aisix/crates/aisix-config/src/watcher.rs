use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use arc_swap::ArcSwap;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{info, warn};

use crate::{
    SnapshotCompileReport,
    etcd::EtcdStore,
    loader::compile_snapshot_from_entries,
    snapshot::CompiledSnapshot,
    startup::{EtcdConfig, StartupConfig},
};

pub fn initial_snapshot_handle(snapshot: CompiledSnapshot) -> Arc<ArcSwap<CompiledSnapshot>> {
    Arc::new(ArcSwap::from_pointee(snapshot))
}

#[derive(Debug)]
struct SnapshotWatcherInner {
    abort: AbortHandle,
}

impl Drop for SnapshotWatcherInner {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotWatcher(Arc<SnapshotWatcherInner>);

impl SnapshotWatcher {
    pub fn new(abort: AbortHandle) -> Self {
        Self(Arc::new(SnapshotWatcherInner { abort }))
    }

    pub fn abort(&self) {
        self.0.abort.abort();
    }
}

pub async fn load_initial_snapshot(config: &StartupConfig) -> Result<CompiledSnapshot> {
    let mut store = EtcdStore::connect(&config.etcd).await?;
    reload_snapshot(&mut store, &config.etcd).await
}

pub async fn spawn_snapshot_watcher(
    config: EtcdConfig,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
) -> Result<SnapshotWatcher> {
    let revision = Arc::new(AtomicI64::new(snapshot.load().revision));
    let task: JoinHandle<()> = tokio::spawn(async move {
        loop {
            if let Err(error) = watch_once(&config, snapshot.clone(), revision.clone()).await {
                warn!(error = %error, prefix = %config.prefix, "snapshot watcher failed; retrying");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    Ok(SnapshotWatcher::new(task.abort_handle()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchResponseAction {
    Continue,
    Reload,
    ReloadAndReconnect,
}

fn classify_watch_response(response: &etcd_client::WatchResponse) -> Result<WatchResponseAction> {
    if response.canceled() {
        if response.compact_revision() > 0 {
            return Ok(WatchResponseAction::ReloadAndReconnect);
        }

        return Err(anyhow!("etcd watch canceled: {}", response.cancel_reason()));
    }

    if response.created() || response.events().is_empty() {
        return Ok(WatchResponseAction::Continue);
    }

    Ok(WatchResponseAction::Reload)
}

async fn watch_once(
    config: &EtcdConfig,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    revision: Arc<AtomicI64>,
) -> Result<()> {
    let mut store = EtcdStore::connect(config).await?;
    let start_revision = Some(revision.load(Ordering::Acquire).saturating_add(1));
    let mut stream = store.watch_prefix(&config.prefix, start_revision).await?;

    loop {
        let response = stream
            .message()
            .await
            .context("failed to receive etcd watch event")?
            .ok_or_else(|| anyhow!("etcd watch stream closed"))?;

        match classify_watch_response(&response)? {
            WatchResponseAction::Continue => continue,
            WatchResponseAction::Reload => {
                if drain_watch_burst(&mut stream).await? == WatchResponseAction::ReloadAndReconnect {
                    reload_snapshot_and_revision(config, snapshot.clone(), revision.clone()).await?;
                    return Ok(());
                }

                reload_snapshot_and_revision(config, snapshot.clone(), revision.clone()).await?;
            }
            WatchResponseAction::ReloadAndReconnect => {
                reload_snapshot_and_revision(config, snapshot.clone(), revision.clone()).await?;
                return Ok(());
            }
        }
    }
}

async fn drain_watch_burst(stream: &mut etcd_client::WatchStream) -> Result<WatchResponseAction> {
    let debounce = tokio::time::sleep(Duration::from_millis(50));
    tokio::pin!(debounce);

    loop {
        tokio::select! {
            _ = &mut debounce => return Ok(WatchResponseAction::Reload),
            response = stream.message() => {
                let Some(response) = response.context("failed to receive etcd watch burst")? else {
                    return Ok(WatchResponseAction::Reload);
                };

                match classify_watch_response(&response)? {
                    WatchResponseAction::Continue => {}
                    WatchResponseAction::Reload => {
                        debounce.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(50));
                    }
                    WatchResponseAction::ReloadAndReconnect => {
                        return Ok(WatchResponseAction::ReloadAndReconnect);
                    }
                }
            }
        }
    }
}

async fn reload_snapshot_and_revision(
    config: &EtcdConfig,
    snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    revision: Arc<AtomicI64>,
) -> Result<()> {
    let mut store = EtcdStore::connect(config).await?;
    let (entries, new_revision) = store.load_prefix(&config.prefix).await?;

    let report = compile_snapshot_from_entries(&config.prefix, &entries, new_revision)
        .map_err(anyhow::Error::msg)?;
    log_snapshot_compile_report(&config.prefix, new_revision, "reloaded", &report);
    snapshot.store(Arc::new(report.snapshot));
    revision.store(new_revision, Ordering::Release);
    Ok(())
}

async fn reload_snapshot(store: &mut EtcdStore, config: &EtcdConfig) -> Result<CompiledSnapshot> {
    let (entries, revision) = store.load_prefix(&config.prefix).await?;
    let report = compile_snapshot_from_entries(&config.prefix, &entries, revision)
        .map_err(anyhow::Error::msg)?;
    log_snapshot_compile_report(&config.prefix, revision, "loaded", &report);
    Ok(report.snapshot)
}

fn log_snapshot_compile_report(
    prefix: &str,
    revision: i64,
    action: &'static str,
    report: &SnapshotCompileReport,
) {
    if report.issues.is_empty() {
        info!(prefix = %prefix, revision, "snapshot {action} successfully");
        return;
    }

    info!(
        prefix = %prefix,
        revision,
        skipped_resources = report.issues.len(),
        "snapshot {action} successfully with skipped resources"
    );

    for issue in &report.issues {
        warn!(
            prefix = %prefix,
            revision,
            resource_kind = issue.kind,
            resource_id = %issue.id,
            reason = %issue.reason,
            "skipped invalid resource during snapshot compile"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{WatchResponseAction, classify_watch_response};

    fn watch_response(
        canceled: bool,
        created: bool,
        compact_revision: i64,
        event_count: usize,
    ) -> etcd_client::WatchResponse {
        etcd_client::WatchResponse(etcd_client::proto::PbWatchResponse {
            canceled,
            created,
            compact_revision,
            cancel_reason: if canceled {
                "compacted revision".to_string()
            } else {
                String::new()
            },
            events: (0..event_count)
                .map(|_| etcd_client::proto::PbEvent::default())
                .collect(),
            ..Default::default()
        })
    }

    #[test]
    fn classifies_compaction_cancel_as_reload_and_reconnect() {
        let response = watch_response(true, false, 42, 0);

        let action = classify_watch_response(&response).expect("compaction should not be an error");

        assert!(matches!(action, WatchResponseAction::ReloadAndReconnect));
    }

    #[test]
    fn classifies_eventful_response_as_reload() {
        let response = watch_response(false, false, 0, 1);

        let action = classify_watch_response(&response).expect("event response should classify");

        assert!(matches!(action, WatchResponseAction::Reload));
    }
}

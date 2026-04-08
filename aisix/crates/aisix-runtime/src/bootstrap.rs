use anyhow::Result;
use aisix_config::{
    startup::StartupConfig,
    watcher::{initial_snapshot_handle, load_initial_snapshot, spawn_snapshot_watcher},
};
use aisix_core::AppState;
use aisix_storage::RedisPool;
use tracing::info;

pub async fn bootstrap(config: &StartupConfig) -> Result<AppState> {
    log_loading_initial_snapshot(&config.etcd.prefix, config.etcd.endpoints.len());
    let snapshot = load_initial_snapshot(config).await?;
    log_initial_snapshot_loaded(snapshot.revision);
    let snapshot = initial_snapshot_handle(snapshot);
    let redis = RedisPool::from_url(&config.redis.url)?;
    log_redis_pool_initialized();
    let watcher = spawn_snapshot_watcher(config.etcd.clone(), snapshot.clone()).await?;
    log_snapshot_watcher_started(&config.etcd.prefix);

    Ok(AppState::with_redis_and_watcher(
        snapshot.clone(),
        true,
        Some(redis),
        Some(watcher),
    ))
}

fn log_loading_initial_snapshot(etcd_prefix: &str, endpoint_count: usize) {
    info!(etcd_prefix, endpoint_count, "loading initial snapshot");
}

fn log_initial_snapshot_loaded(revision: i64) {
    info!(revision, "initial snapshot loaded");
}

fn log_redis_pool_initialized() {
    info!("redis pool initialized");
}

fn log_snapshot_watcher_started(etcd_prefix: &str) {
    info!(etcd_prefix, "snapshot watcher started");
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    use super::{
        log_initial_snapshot_loaded, log_loading_initial_snapshot, log_redis_pool_initialized,
        log_snapshot_watcher_started,
    };

    #[test]
    fn bootstrap_logs_snapshot_progress() {
        let output = capture_logs(|| {
            log_loading_initial_snapshot("/aisix", 2);
            log_initial_snapshot_loaded(42);
        });

        assert!(output.contains("loading initial snapshot"));
        assert!(output.contains("/aisix"));
        assert!(output.contains("endpoint_count=2"));
        assert!(output.contains("initial snapshot loaded"));
        assert!(output.contains("revision=42"));
    }

    #[test]
    fn bootstrap_logs_redis_and_watcher_readiness() {
        let output = capture_logs(|| {
            log_redis_pool_initialized();
            log_snapshot_watcher_started("/aisix");
        });

        assert!(output.contains("redis pool initialized"));
        assert!(output.contains("snapshot watcher started"));
        assert!(output.contains("/aisix"));
    }

    fn capture_logs(run: impl FnOnce()) -> String {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_writer(TestWriter(buffer.clone()))
            .finish();

        with_default(subscriber, run);

        let captured = buffer.lock().unwrap().clone();
        String::from_utf8(captured).unwrap()
    }

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for TestWriter {
        type Writer = TestWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            TestWriterGuard(self.0.clone())
        }
    }

    struct TestWriterGuard(Arc<Mutex<Vec<u8>>>);

    impl io::Write for TestWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

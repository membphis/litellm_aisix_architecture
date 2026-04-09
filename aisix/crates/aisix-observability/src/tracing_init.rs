use std::sync::{Mutex, OnceLock};

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(level: &str) -> anyhow::Result<()> {
    init_with_state(global_init_state(), level, init_subscriber)
}

fn init_subscriber(config: &TracingConfig) -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .json()
                .with_current_span(false)
                .with_span_list(false),
        )
        .with(config.filter.clone())
        .try_init()
        .map_err(Into::into)
}

fn init_with_state(
    init_state: &InitCell,
    level: &str,
    initialize: impl FnOnce(&TracingConfig) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let config = build_effective_config(level);
    let mut state = init_state.state.lock().unwrap();

    if let Some(existing) = state.as_ref() {
        return existing.validate(&config.signature);
    }

    let initialized = InitState::from_result(config.signature.clone(), initialize(&config));
    let result = initialized.validate(&config.signature);
    *state = Some(initialized);

    result
}

fn global_init_state() -> &'static InitCell {
    static INIT_STATE: OnceLock<InitCell> = OnceLock::new();
    INIT_STATE.get_or_init(InitCell::default)
}

fn build_effective_config(level: &str) -> TracingConfig {
    let filter = build_env_filter(&std::env::var("RUST_LOG").unwrap_or_default(), level);

    TracingConfig {
        signature: TracingConfigSignature {
            filter_directives: filter.to_string(),
            json_output: true,
        },
        filter,
    }
}

fn build_env_filter(env_filter: &str, level: &str) -> EnvFilter {
    if !env_filter.trim().is_empty() {
        if let Ok(filter) = EnvFilter::try_new(env_filter) {
            return filter;
        }
    }

    EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"))
}

#[derive(Clone, Debug)]
struct TracingConfig {
    signature: TracingConfigSignature,
    filter: EnvFilter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TracingConfigSignature {
    filter_directives: String,
    json_output: bool,
}

struct InitState {
    signature: TracingConfigSignature,
    init_error: Option<String>,
}

#[derive(Default)]
struct InitCell {
    state: Mutex<Option<InitState>>,
}

impl InitState {
    fn from_result(signature: TracingConfigSignature, result: anyhow::Result<()>) -> Self {
        Self {
            signature,
            init_error: result.err().map(|error| error.to_string()),
        }
    }

    fn validate(&self, requested: &TracingConfigSignature) -> anyhow::Result<()> {
        if self.signature != *requested {
            return Err(anyhow::anyhow!(format!(
                "tracing already initialized with different config: existing={:?}, requested={:?}",
                self.signature, requested
            )));
        }

        match &self.init_error {
            Some(error) => Err(anyhow::Error::msg(error.clone())),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, OnceLock};

    use tracing_subscriber::EnvFilter;

    use super::{build_effective_config, init_with_state};

    #[test]
    fn init_runs_initializer_once_and_allows_same_effective_config() {
        let _guard = test_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("RUST_LOG", None);
        let state = super::InitCell::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        init_with_state(&state, "info", {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap();

        init_with_state(&state, "info", |_| -> anyhow::Result<()> {
            panic!("initializer should not run twice for same config")
        })
        .unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn init_uses_rust_log_over_passed_level() {
        let _guard = test_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("RUST_LOG", Some("debug,hyper=warn"));
        let state = super::InitCell::default();
        let seen_signature = Arc::new(Mutex::new(None));

        init_with_state(&state, "info", {
            let seen_signature = seen_signature.clone();
            move |config| {
                *seen_signature.lock().unwrap() = Some(config.signature.clone());
                Ok(())
            }
        })
        .unwrap();

        init_with_state(&state, "trace", |_| -> anyhow::Result<()> {
            panic!("effective config should still match RUST_LOG")
        })
        .unwrap();

        assert_eq!(
            seen_signature.lock().unwrap().clone().unwrap(),
            super::TracingConfigSignature {
                filter_directives: EnvFilter::new("debug,hyper=warn").to_string(),
                json_output: true,
            }
        );
    }

    #[test]
    fn init_rejects_different_effective_config_after_first_call() {
        let _guard = test_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("RUST_LOG", None);
        let state = super::InitCell::default();

        init_with_state(&state, "trace", |_| Ok(())).unwrap();

        let error = init_with_state(&state, "info", |_| Ok(())).unwrap_err();
        assert!(error
            .to_string()
            .contains("tracing already initialized with different config"));
    }

    #[test]
    fn concurrent_calls_publish_only_the_config_that_initialized() {
        let _guard = test_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("RUST_LOG", None);
        let state = std::sync::Arc::new(super::InitCell::default());
        let trace_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let info_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        std::thread::scope(|scope| {
            let left = scope.spawn({
                let state = state.clone();
                let trace_calls = trace_calls.clone();
                move || {
                    init_with_state(&state, "trace", |config| {
                        trace_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        assert_eq!(config.signature.filter_directives, "trace");
                        Ok(())
                    })
                }
            });

            let right = scope.spawn({
                let state = state.clone();
                let info_calls = info_calls.clone();
                move || {
                    init_with_state(&state, "info", |config| {
                        info_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        assert_eq!(config.signature.filter_directives, "info");
                        Ok(())
                    })
                }
            });

            let left = left.join().unwrap();
            let right = right.join().unwrap();

            match (left, right) {
                (Ok(()), Err(error)) | (Err(error), Ok(())) => {
                    assert!(error
                        .to_string()
                        .contains("tracing already initialized with different config"));
                }
                other => panic!("unexpected concurrent init outcome: {other:?}"),
            }
        });

        assert_eq!(
            trace_calls.load(std::sync::atomic::Ordering::SeqCst)
                + info_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn init_succeeds_on_first_local_call() {
        let _guard = test_env_lock().lock().unwrap();
        let _env = EnvVarGuard::set("RUST_LOG", Some("aisix_observability=info"));
        let state = super::InitCell::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        init_with_state(&state, "trace", {
            let calls = calls.clone();
            move |config| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert_eq!(
                    config.signature.filter_directives,
                    "aisix_observability=info"
                );
                Ok(())
            }
        })
        .unwrap();

        init_with_state(&state, "debug", |_| -> anyhow::Result<()> {
            panic!("initializer should not rerun after first successful init")
        })
        .unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn falls_back_to_requested_level_when_env_filter_is_invalid() {
        let _guard = test_env_lock().lock().unwrap();
        let invalid = "[not-valid";
        assert!(EnvFilter::try_new(invalid).is_err());

        let _env = EnvVarGuard::set("RUST_LOG", Some(invalid));
        let config = build_effective_config("info");

        assert_eq!(
            config.filter.to_string(),
            EnvFilter::new("info").to_string()
        );
    }

    fn test_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(name).ok();
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }

            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var(self.name, previous),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

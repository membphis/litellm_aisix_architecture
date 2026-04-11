use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use aisix_types::error::{ErrorKind, GatewayError};

#[derive(Debug, Clone, Default)]
pub struct ShadowLimiter {
    windows: Arc<Mutex<HashMap<String, MinuteWindow>>>,
}

#[derive(Debug, Clone, Copy)]
struct MinuteWindow {
    bucket: u64,
    count: u64,
}

impl ShadowLimiter {
    pub fn check_rpm(&self, key: &str, limit: u64) -> Result<(), GatewayError> {
        let bucket = current_minute_bucket();
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let window = windows
            .entry(key.to_string())
            .or_insert(MinuteWindow { bucket, count: 0 });
        if window.bucket != bucket {
            window.bucket = bucket;
            window.count = 0;
        }
        if window.count >= limit {
            return Err(rate_limited());
        }

        window.count += 1;
        Ok(())
    }
}

pub(crate) fn current_minute_bucket() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 60
}

pub(crate) fn rate_limited() -> GatewayError {
    GatewayError {
        kind: ErrorKind::RateLimited,
        message: "request rate limit exceeded".to_string(),
    }
}

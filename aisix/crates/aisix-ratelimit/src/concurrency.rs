use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use aisix_types::error::{ErrorKind, GatewayError};

#[derive(Debug, Clone, Default)]
pub struct ConcurrencyLimiter {
    in_flight: Arc<Mutex<HashMap<String, u64>>>,
}

#[derive(Debug)]
pub struct ConcurrencyGuard {
    key: Option<String>,
    in_flight: Arc<Mutex<HashMap<String, u64>>>,
}

impl ConcurrencyLimiter {
    pub fn acquire(
        &self,
        key: String,
        limit: Option<u64>,
    ) -> Result<ConcurrencyGuard, GatewayError> {
        let Some(limit) = limit else {
            return Ok(ConcurrencyGuard {
                key: None,
                in_flight: self.in_flight.clone(),
            });
        };

        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = in_flight.entry(key.clone()).or_insert(0);
        if *current >= limit {
            return Err(GatewayError {
                kind: ErrorKind::RateLimited,
                message: "concurrency limit exceeded".to_string(),
            });
        }
        *current += 1;

        Ok(ConcurrencyGuard {
            key: Some(key),
            in_flight: self.in_flight.clone(),
        })
    }
}

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        let Some(key) = &self.key else {
            return;
        };

        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = in_flight.get_mut(key) {
            *current = current.saturating_sub(1);
            if *current == 0 {
                in_flight.remove(key);
            }
        }
    }
}

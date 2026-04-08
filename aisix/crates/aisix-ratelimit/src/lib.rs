//! Task 2 establishes the module layout only; implementations will be added in later tasks.

pub mod concurrency;
pub mod redis_check;
pub mod service;
pub mod shadow;

pub use concurrency::{ConcurrencyGuard, ConcurrencyLimiter};
pub use service::RateLimitService;

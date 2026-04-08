//! Task 2 establishes the module layout only; implementations will be added in later tasks.

pub mod counters;
pub mod redis_pool;

pub use counters::CounterStore;
pub use redis_pool::{RedisError, RedisPool};

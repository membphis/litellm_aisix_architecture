//! Task 2 establishes the module layout only; implementations will be added in later tasks.

pub mod metrics;
pub mod tracing_init;

pub use metrics::{encode as encode_metrics, register as register_collector, shared_registry};

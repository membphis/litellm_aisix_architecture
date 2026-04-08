//! Task 2 establishes the module layout only; implementations will be added in later tasks.

pub mod anthropic;
pub mod anthropic_sse;
pub mod codec;
pub mod openai_compat;
pub mod openai_sse;
pub mod registry;

pub use codec::{JsonOutput, ProviderCodec, StreamOutput};
pub use openai_compat::OpenAiCompatCodec;
pub use registry::ProviderRegistry;

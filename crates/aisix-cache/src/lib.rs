//! Task 2 establishes the module layout only; implementations will be added in later tasks.

pub mod key;
pub mod memory;

pub use key::build_chat_cache_key;
pub use memory::{CachedChatResponse, MemoryCache};

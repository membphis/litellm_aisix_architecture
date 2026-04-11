use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use aisix_types::usage::Usage;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct CachedChatResponse {
    pub body: Bytes,
    pub provider_id: String,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryCache {
    entries: Arc<RwLock<HashMap<String, CachedChatResponse>>>,
}

impl MemoryCache {
    pub fn get_chat(&self, key: &str) -> Option<CachedChatResponse> {
        self.entries.read().ok()?.get(key).cloned()
    }

    pub fn put_chat(&self, key: String, value: CachedChatResponse) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(key, value);
        }
    }
}

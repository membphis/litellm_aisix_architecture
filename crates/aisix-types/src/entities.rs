use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMeta {
    pub key_id: String,
    pub user_id: Option<String>,
    pub customer_id: Option<String>,
    pub alias: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub allowed_models: Vec<String>,
}

use aisix_types::usage::Usage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEvent {
    pub key_id: String,
    pub model: String,
    pub usage: Usage,
}

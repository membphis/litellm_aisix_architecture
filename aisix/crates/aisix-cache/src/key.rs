pub fn build_chat_cache_key(
    snapshot_revision: i64,
    provider_id: &str,
    upstream_model: &str,
    model: &str,
    request_signature: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&(
        snapshot_revision,
        provider_id,
        upstream_model,
        model,
        request_signature,
    ))
}

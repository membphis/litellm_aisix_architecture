# 实体 Schema

### Policy（可复用限流模板）

```json
{
  "id": "standard-tier",
  "rate_limit": {
    "rpm": 500,
    "rpd": 5000,
    "tpm": 100000,
    "tpd": 1000000,
    "concurrency": 10
  }
}
```

### Provider

```json
{
  "id": "openai-us",
  "kind": "openai",
  "base_url": "https://api.openai.com",
  "auth": { "secret_ref": "env:OPENAI_API_KEY" },
  "policy_id": "standard-tier"
}
```

`kind` 决定使用哪个 `ProviderCodec` 实现：
- `openai` → `OpenAICompatCodec`（覆盖 OpenAI、Azure、Ollama、vLLM、Groq）
- `anthropic`、`vertex`、`bedrock` → 独立 codec 实现

### Model

```json
{
  "id": "gpt-4o-mini",
  "provider_id": "openai-us",
  "upstream_model": "gpt-4.1-mini",
  "policy_id": "standard-tier"
}
```

### API Key

```json
{
  "id": "key-abc123",
  "key": "my-secret-key",
  "allowed_models": ["gpt-4o-mini", "claude-sonnet"],
  "policy_id": "standard-tier",
  "rate_limit": { "rpm": 100, "tpm": 50000 }
}
```

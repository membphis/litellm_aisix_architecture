# aisix

Minimal AI gateway with a built-in Admin API for phase 1.

## Getting Started

1. Start dependencies:

```bash
docker compose up -d redis etcd
```

2. Set the upstream provider secret used by the example provider config:

```bash
export OPENAI_API_KEY="your-openai-key"
```

3. Start the gateway with the example startup config:

```bash
cargo run --manifest-path Cargo.toml -p aisix-gateway -- config/aisix-gateway.example.yaml
```

4. Create a provider through the embedded Admin API:

```bash
curl -fsS -X PUT http://127.0.0.1:4000/admin/providers/openai \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "openai",
    "kind": "openai",
    "base_url": "https://api.openai.com",
    "auth": {"secret_ref": "env:OPENAI_API_KEY"},
    "policy_id": null,
    "rate_limit": null
  }'
```

5. Create a chat model and an embeddings model:

```bash
curl -fsS -X PUT http://127.0.0.1:4000/admin/models/gpt-4o-mini \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "gpt-4o-mini",
    "provider_id": "openai",
    "upstream_model": "gpt-4o-mini",
    "policy_id": null,
    "rate_limit": null
  }'

curl -fsS -X PUT http://127.0.0.1:4000/admin/models/text-embedding-3-small \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "text-embedding-3-small",
    "provider_id": "openai",
    "upstream_model": "text-embedding-3-small",
    "policy_id": null,
    "rate_limit": null
  }'
```

6. Create a virtual API key allowed to use both models:

```bash
curl -fsS -X PUT http://127.0.0.1:4000/admin/apikeys/demo-key \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "demo-key",
    "key": "sk-demo-phase1",
    "allowed_models": ["gpt-4o-mini", "text-embedding-3-small"],
    "policy_id": null,
    "rate_limit": null
  }'
```

7. Call chat:

```bash
curl -fsS http://127.0.0.1:4000/v1/chat/completions \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer sk-demo-phase1' \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Say hello in one sentence."}],
    "stream": false
  }'
```

8. Call embeddings:

```bash
curl -fsS http://127.0.0.1:4000/v1/embeddings \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer sk-demo-phase1' \
  -d '{
    "model": "text-embedding-3-small",
    "input": "hello from aisix"
  }'
```

## Smoke Script

Run `./scripts/smoke-phase1.sh` after the gateway is up. It checks health, writes one provider/model/apikey through the Admin API, and sends one chat request through the gateway.

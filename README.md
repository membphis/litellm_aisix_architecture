# aisix

[![CI](https://github.com/membphis/litellm_aisix_architecture/actions/workflows/ci.yml/badge.svg)](https://github.com/membphis/litellm_aisix_architecture/actions/workflows/ci.yml)

Minimal AI gateway with a built-in Admin API for phase 1.

## Getting Started

1. Start dependencies:

```bash
docker compose -f docker-compose.yml up -d redis etcd
```

2. Set the upstream provider secret used by the example provider config:

```bash
export OPENAI_API_KEY="your-openai-key"
```

Any OpenAI-compatible upstream also works. For example, with DeepSeek you can keep `kind: "openai"` and override the smoke script inputs instead of changing gateway code:

```bash
export DEEPSEEK_API_KEY="your-deepseek-key"
export AISIX_PROVIDER_BASE_URL="https://api.deepseek.com"
export AISIX_PROVIDER_SECRET_REF="env:DEEPSEEK_API_KEY"
export AISIX_UPSTREAM_MODEL="deepseek-chat"
```

3. Start the gateway with the example startup config:

```bash
cargo run -p aisix-gateway -- config/aisix-gateway.example.yaml
```

The example config uses two HTTP listeners:

- data plane: `server.listen` (`0.0.0.0:4000`)
- admin plane: `server.admin_listen` (`127.0.0.1:4001`)

The Admin API and Admin UI always share the same admin port.
That admin port must be different from the data plane port.

Gateway startup now depends on reachable etcd. The gateway loads its initial runtime snapshot from etcd under the configured prefix and fails to start if etcd cannot be reached.

The embedded Admin API writes config into etcd under the configured prefix. Runtime changes are applied asynchronously by the background etcd watcher after the new full snapshot compiles successfully. A successful Admin response means etcd accepted the write; it does not guarantee the new config is already active.

If a write stores invalid config that later fails compilation, the Admin request may still succeed because etcd accepted it, while the runtime keeps serving the previous compiled snapshot.

4. Open the embedded Admin UI when you want a browser control plane:

```text
http://127.0.0.1:4001/ui
```

When prompted, enter the admin key manually.
The browser stores it only in `sessionStorage` for the current session and discards it when the browser is closed.

The admin listener also exposes the control-plane OpenAPI contract at:

```text
http://127.0.0.1:4001/openapi/admin.json
```

The embedded UI consumes that same OpenAPI document as its Admin resource schema source.

5. Create a provider through the embedded Admin API:

```bash
curl -fsS -X PUT http://127.0.0.1:4001/admin/providers/openai \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "openai",
    "kind": "openai",
    "base_url": "https://api.openai.com",
    "auth": {"secret_ref": "env:OPENAI_API_KEY"}
  }'
```

Admin `PUT` requests are checked against the OpenAPI-backed request schema before AISIX stores them. Unknown fields, missing required fields, and invalid enum values are rejected with `400 Bad Request`.

6. Create a chat model and an embeddings model:

```bash
curl -fsS -X PUT http://127.0.0.1:4001/admin/models/gpt-4o-mini \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "gpt-4o-mini",
    "provider_id": "openai",
    "upstream_model": "gpt-4o-mini"
  }'

curl -fsS -X PUT http://127.0.0.1:4001/admin/models/text-embedding-3-small \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "text-embedding-3-small",
    "provider_id": "openai",
    "upstream_model": "text-embedding-3-small"
  }'
```

7. Create a virtual API key allowed to use both models:

```bash
curl -fsS -X PUT http://127.0.0.1:4001/admin/apikeys/demo-key \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "demo-key",
    "key": "sk-demo-phase1",
    "allowed_models": ["gpt-4o-mini", "text-embedding-3-small"]
  }'
```

8. Call chat after the watcher has reloaded the updated snapshot:

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

9. Call embeddings:

```bash
curl -fsS http://127.0.0.1:4000/v1/embeddings \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer sk-demo-phase1' \
  -d '{
    "model": "text-embedding-3-small",
    "input": "hello from aisix"
  }'
```

## Cache Policy

- Global default cache behavior is configured in `config/aisix-gateway.example.yaml` via `cache.default`.
- Supported startup values are `enabled` and `disabled`.
- The global default is `disabled` when omitted.
- Provider and model resources may set `cache.mode` to `inherit`, `enabled`, or `disabled`.
- Missing `cache` is treated as `inherit`.
- Effective precedence is `model > provider > global default`.
- Current response caching applies only to non-stream chat JSON requests.
- When caching is disabled for the request, AISIX skips both cache lookup and cache store, and does not return `x-aisix-cache-hit`.
- When caching is enabled but the request misses cache, AISIX returns `x-aisix-cache-hit: false`.
- When caching is enabled and the request hits cache, AISIX returns `x-aisix-cache-hit: true`.
- The current design does not support API key-level static cache switches.
- The current design does not attach cache policy to `policy` resources.

## Smoke Script

Run `bash scripts/smoke-phase1.sh` after the gateway is up. It exercises the etcd-backed flow by checking health, writing one provider/model/apikey through the Admin API, and sending one chat request through the gateway. The script defaults to OpenAI, but it also accepts OpenAI-compatible overrides via `AISIX_PROVIDER_ID`, `AISIX_PROVIDER_BASE_URL`, `AISIX_PROVIDER_SECRET_REF`, and `AISIX_UPSTREAM_MODEL` so you can point it at DeepSeek or another compatible upstream. Admin success in that flow means the write reached etcd; the new config becomes active only after the background watcher reloads a successfully compiled snapshot.

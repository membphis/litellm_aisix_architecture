# Admin API

## Overview

The Admin API manages runtime config stored in etcd under the configured prefix.

The Admin API is served on the dedicated admin listener (`server.admin_listen`).
The embedded Admin UI at `/ui` uses that same admin port.
That admin port must not overlap with the data plane listener (`server.listen`).

The runtime snapshot is compiled from the valid subset of resources currently stored under that prefix.
Resources with invalid dependencies are skipped and treated as absent from the live runtime until they are fixed.
Other valid resources continue to compile and apply normally.

All Admin requests require:

- header `x-admin-key: <admin-key>`
- `content-type: application/json` for `PUT`

Admin writes are accepted by etcd first and applied to the live gateway later by the background watcher.
This means a successful `PUT` or `DELETE` response confirms the config change was stored in etcd, not that the new runtime snapshot is already active.

This API is machine-facing and supports concurrent writes across related resources.
For example, a `model` write may arrive before the referenced `provider` write. The Admin API accepts that ordering and leaves final runtime convergence to the watcher.

The browser UI is only a human-facing companion to this API. Operators enter the admin key manually in the UI, and the browser keeps it only in `sessionStorage` for the current session.

## Resources

The Admin API currently supports four collections:

- `providers`
- `models`
- `apikeys`
- `policies`

Each collection supports:

- `GET /admin/<collection>`
- `GET /admin/<collection>/:id`
- `PUT /admin/<collection>/:id`
- `DELETE /admin/<collection>/:id`

## Common Semantics

### Resource IDs

- The `:id` path segment must match the JSON body `id` for `PUT`.
- Resource IDs must not contain `/`.
- Collection `GET` responses are sorted by `id` ascending.

### Success Codes

- `GET /admin/<collection>` returns `200 OK` with a JSON array.
- `GET /admin/<collection>/:id` returns `200 OK` with the stored JSON object.
- `PUT /admin/<collection>/:id` returns `200 OK` with a write result.
- `DELETE /admin/<collection>/:id` returns `200 OK` with a delete result.

### Error Codes

- `401 Unauthorized`: missing or invalid `x-admin-key`
- `400 Bad Request`: invalid resource id or path/body id mismatch
- `404 Not Found`: missing resource on item `GET` or `DELETE`
- `500 Internal Server Error`: etcd or server-side failure

## Response Shapes

### Write/Delete Result

`PUT` and successful `DELETE` both return:

```json
{
  "id": "openai",
  "path": "/aisix/providers/openai",
  "revision": 123
}
```

### Collection Result

Example `GET /admin/providers` response:

```json
[
  {
    "id": "anthropic",
    "kind": "anthropic",
    "base_url": "https://api.anthropic.com",
    "auth": { "secret_ref": "env:ANTHROPIC_API_KEY" }
  },
  {
    "id": "openai",
    "kind": "openai",
    "base_url": "https://api.openai.com",
    "auth": { "secret_ref": "env:OPENAI_API_KEY" }
  }
]
```

### API Key Reads

`apikey` reads return the stored `key` field in plaintext.
This is intentional in the current API contract.

## Examples

### List Providers

```bash
curl -fsS http://127.0.0.1:4001/admin/providers \
  -H 'x-admin-key: change-me-admin-key'
```

### Get One Provider

```bash
curl -fsS http://127.0.0.1:4001/admin/providers/openai \
  -H 'x-admin-key: change-me-admin-key'
```

### Put One Provider

```bash
curl -fsS -X PUT http://127.0.0.1:4001/admin/providers/openai \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "openai",
    "kind": "openai",
    "base_url": "https://api.openai.com",
    "auth": {"secret_ref": "env:OPENAI_API_KEY"},
    "cache": {"mode": "enabled"}
  }'
```

### Delete One Provider

```bash
curl -fsS -X DELETE http://127.0.0.1:4001/admin/providers/openai \
  -H 'x-admin-key: change-me-admin-key'
```

### Put a Model Before Its Provider Exists

This is allowed by design for machine clients that issue related writes concurrently:

```bash
curl -fsS -X PUT http://127.0.0.1:4001/admin/models/gpt-4o-mini \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "gpt-4o-mini",
    "provider_id": "openai",
    "upstream_model": "gpt-4o-mini",
    "cache": {"mode": "inherit"}
  }'
```

The write can succeed even if `openai` has not been written yet.
Until that provider exists, the model is dependency-invalid and absent from the runtime snapshot.
Other valid resources continue to apply while this model is skipped.
Once `openai` is written, a later reload includes `gpt-4o-mini` automatically.

### List API Keys

```bash
curl -fsS http://127.0.0.1:4001/admin/apikeys \
  -H 'x-admin-key: change-me-admin-key'
```

### Get One API Key

```bash
curl -fsS http://127.0.0.1:4001/admin/apikeys/demo-key \
  -H 'x-admin-key: change-me-admin-key'
```

### Delete One API Key

```bash
curl -fsS -X DELETE http://127.0.0.1:4001/admin/apikeys/demo-key \
  -H 'x-admin-key: change-me-admin-key'
```

## Resource Schemas

### Provider

```json
{
  "id": "openai",
  "kind": "openai",
  "base_url": "https://api.openai.com",
  "auth": { "secret_ref": "env:OPENAI_API_KEY" },
  "cache": { "mode": "enabled" }
}
```

`cache` is optional. When omitted, provider cache behavior defaults to `inherit`.

### Model

```json
{
  "id": "gpt-4o-mini",
  "provider_id": "openai",
  "upstream_model": "gpt-4o-mini",
  "cache": { "mode": "inherit" }
}
```

`cache` is optional. Supported `cache.mode` values are `inherit`, `enabled`, and `disabled`. Missing `cache` is treated as `inherit`.

## Cache Policy

- The startup YAML exposes the process-level default via `cache.default`.
- Provider and model resources expose resource-level overrides via `cache.mode`.
- Effective precedence is `model > provider > global default`.
- The global default is `disabled` when omitted.
- Static cache switches are not supported on API keys.
- Cache policy is not attached to `policy` resources.
- The current cache policy applies only to non-stream chat JSON requests.

### API Key

```json
{
  "id": "demo-key",
  "key": "sk-demo-phase1",
  "allowed_models": ["gpt-4o-mini"]
}
```

### Policy

```json
{
  "id": "default",
  "rate_limit": {
    "rpm": 60
  }
}
```

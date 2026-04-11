# AISIX Project

A Rust AI Gateway (data plane) with etcd-backed config sync. The repository root is the Cargo workspace root, alongside docs and project metadata.

## Workspace Layout

```
bin/aisix-gateway/              # Gateway binary entrypoint (main.rs)
crates/
  aisix-types/                  # Shared types: entities, request/response, stream events, errors
  aisix-core/                   # App state (ArcSwap snapshot holder), request context
  aisix-config/                 # etcd loader, watcher, config compilation, snapshot
  aisix-server/                 # axum HTTP server: admin API, handlers, pipeline, stream proxy
  aisix-auth/                   # Virtual key authentication
  aisix-policy/                 # Policy engine
  aisix-ratelimit/              # Rate limiting (Redis-backed)
  aisix-cache/                  # Response caching
  aisix-router/                 # Model routing / load balancing
  aisix-providers/              # Upstream provider codecs (OpenAI, etc.)
  aisix-spend/                  # Usage / spend tracking
  aisix-storage/                # etcd / Redis client abstractions
  aisix-observability/          # Metrics, logging
  aisix-runtime/                # Bootstrap: wires crates together, starts server + watcher
config/                         # Example gateway YAML config
scripts/smoke-phase1.sh         # End-to-end smoke test
```

## Developer Commands

All Cargo commands run directly from the repository root:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo run -p aisix-gateway -- config/aisix-gateway.example.yaml
```

There is no separate lint/typecheck command; use `cargo clippy` and `cargo test`.

## Prerequisites

- **etcd** and **Redis** must be running before starting the gateway:
  ```bash
  docker compose -f docker-compose.yml up -d redis etcd
  ```
- The gateway fails to start if etcd is unreachable (it loads initial snapshot from etcd).
- Set `OPENAI_API_KEY` env var for upstream provider auth when using the example config.

## Key Architecture Concepts

- **Immutable compiled snapshot + ArcSwap**: Config hot-reload with zero downtime. The watcher compiles a new snapshot from etcd and atomically swaps it in.
- **Admin API writes to etcd, not runtime**: A successful Admin `PUT`/`DELETE` means etcd accepted the write. The background watcher applies it asynchronously. Invalid config that fails compilation does NOT affect the running snapshot.
- **Admin auth**: All admin requests require `x-admin-key` header matching the config value.
- **Admin listener split**: The Admin API and Admin UI share `server.admin_listen`; that admin port must not overlap with the data plane `server.listen` port.
- **Admin key browser handling**: The Admin UI requires operator-entered admin keys and stores them only in browser session scope.
- **Config prefix**: All etcd keys live under the configured prefix (default `/aisix`).

## Testing

```bash
cargo test                    # all tests
cargo test -p aisix-config    # single crate
```

For smoke testing a running gateway:
```bash
bash scripts/smoke-phase1.sh
```

## Docs

- `README.md` — getting started guide with curl examples
- `docs/admin-api.md` — Admin API reference (resources, semantics, error codes)
- `docs/architecture.md` — full architecture design doc (Chinese)
- `docs/litellm-feature-panorama.md` — LiteLLM feature panorama used as reference baseline

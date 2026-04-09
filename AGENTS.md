# AISIX Project

A Rust AI Gateway (data plane) with etcd-backed config sync. The repo root holds design docs and diagrams; the actual Rust workspace is under `aisix/`.

## Workspace Layout

```
aisix/                          # Rust Cargo workspace
  bin/aisix-gateway/            # Gateway binary entrypoint (main.rs)
  crates/
    aisix-types/                # Shared types: entities, request/response, stream events, errors
    aisix-core/                 # App state (ArcSwap snapshot holder), request context
    aisix-config/               # etcd loader, watcher, config compilation, snapshot
    aisix-server/               # axum HTTP server: admin API, handlers, pipeline, stream proxy
    aisix-auth/                 # Virtual key authentication
    aisix-policy/               # Policy engine
    aisix-ratelimit/            # Rate limiting (Redis-backed)
    aisix-cache/                # Response caching
    aisix-router/               # Model routing / load balancing
    aisix-providers/            # Upstream provider codecs (OpenAI, etc.)
    aisix-spend/                # Usage / spend tracking
    aisix-storage/              # etcd / Redis client abstractions
    aisix-observability/        # Metrics, logging
    aisix-runtime/              # Bootstrap: wires crates together, starts server + watcher
  config/                       # Example gateway YAML config
  scripts/smoke-phase1.sh       # End-to-end smoke test
```

## Developer Commands

All Cargo commands must run with `--manifest-path aisix/Cargo.toml` from the repo root:

```bash
cargo build --manifest-path aisix/Cargo.toml
cargo test --manifest-path aisix/Cargo.toml
cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings
cargo run --manifest-path aisix/Cargo.toml -p aisix-gateway -- aisix/config/aisix-gateway.example.yaml
```

There is no separate lint/typecheck command; use `cargo clippy` and `cargo test`.

## Prerequisites

- **etcd** and **Redis** must be running before starting the gateway:
  ```bash
  docker compose -f aisix/docker-compose.yml up -d redis etcd
  ```
- The gateway fails to start if etcd is unreachable (it loads initial snapshot from etcd).
- Set `OPENAI_API_KEY` env var for upstream provider auth when using the example config.

## Key Architecture Concepts

- **Immutable compiled snapshot + ArcSwap**: Config hot-reload with zero downtime. The watcher compiles a new snapshot from etcd and atomically swaps it in.
- **Admin API writes to etcd, not runtime**: A successful Admin `PUT`/`DELETE` means etcd accepted the write. The background watcher applies it asynchronously. Invalid config that fails compilation does NOT affect the running snapshot.
- **Admin auth**: All admin requests require `x-admin-key` header matching the config value.
- **Config prefix**: All etcd keys live under the configured prefix (default `/aisix`).

## Testing

```bash
cargo test --manifest-path aisix/Cargo.toml                    # all tests
cargo test --manifest-path aisix/Cargo.toml -p aisix-config    # single crate
```

For smoke testing a running gateway:
```bash
bash aisix/scripts/smoke-phase1.sh
```

## Docs

- `aisix/README.md` — getting started guide with curl examples
- `aisix/docs/admin-api.md` — Admin API reference (resources, semantics, error codes)
- `docs/architecture.md` — full architecture design doc (Chinese)
- `docs/litellm-feature-panorama.md` — LiteLLM feature panorama used as reference baseline

# AISIX Phase 1 MVP Plan

## Task 1: Initialize the workspace root and local dev environment

Files:
- Create: `aisix/Cargo.toml`
- Create: `aisix/docker-compose.yml`
- Create: `aisix/config/aisix-gateway.example.yaml`
- Create: `aisix/scripts/smoke-phase1.sh`

Step 1 content for `aisix/Cargo.toml`:

```toml
[workspace]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
anyhow = "1"
arc-swap = "1"
async-trait = "0.1"
axum = { version = "0.8", features = ["macros"] }
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
etcd-client = "0.14"
futures = "0.3"
governor = "0.8"
http = "1"
hyper = "1"
moka = { version = "0.12", features = ["future"] }
opentelemetry = "0.27"
opentelemetry-otlp = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
prometheus = "0.13"
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
sha2 = "0.10"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "limit"] }
tracing = "0.1"
tracing-opentelemetry = "0.28"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
uuid = { version = "1", features = ["v4", "serde"] }
wiremock = "0.6"
```

Step 2 content:

`aisix/docker-compose.yml`

```yaml
services:
  etcd:
    image: quay.io/coreos/etcd:v3.5.17
    command:
      - etcd
      - --name=aisix-etcd
      - --listen-client-urls=http://0.0.0.0:2379
      - --advertise-client-urls=http://127.0.0.1:2379
      - --listen-peer-urls=http://0.0.0.0:2380
    ports:
      - "2379:2379"

  redis:
    image: redis:7-alpine
    command: redis-server --save "" --appendonly no
    ports:
      - "6379:6379"
```

`aisix/config/aisix-gateway.example.yaml`

```yaml
server:
  listen: "0.0.0.0:4000"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8

etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000

redis:
  url: "redis://127.0.0.1:6379"

log:
  level: "info"

runtime:
  worker_threads: 0

deployment:
  admin:
    enabled: true
    admin_keys:
      - key: "change-me-admin-key"
```

`aisix/scripts/smoke-phase1.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

curl -fsS http://127.0.0.1:4000/health >/dev/null
curl -fsS http://127.0.0.1:4000/ready >/dev/null
echo "phase1 smoke verified /health and /ready endpoints"
```

Step 3 verification command:

`docker compose -f aisix/docker-compose.yml config >/dev/null && test -x aisix/scripts/smoke-phase1.sh`

Expected: exits 0

## Task 2: Create the initial crate manifests

Task 2 is responsible for creating the actual crate directories and manifests, adding the workspace `members = [...]` list to `aisix/Cargo.toml` once those paths exist, and keeping the initial manifests compile-minimal for the empty skeleton rather than predeclaring speculative runtime dependencies.

## 1. Flatten the workspace root

- [x] 1.1 Move `aisix/Cargo.toml` and `aisix/Cargo.lock` to the repository root and remove the old nested manifest location.
- [x] 1.2 Move `aisix/bin/`, `aisix/crates/`, `aisix/config/`, and `aisix/scripts/` to root-level `bin/`, `crates/`, `config/`, and `scripts/` directories.
- [x] 1.3 Move `aisix/docker-compose.yml` to `docker-compose.yml` at the repository root.
- [x] 1.4 Move the active contributor README from `aisix/README.md` to `README.md` at the repository root.
- [x] 1.5 Move the active admin API documentation from `aisix/docs/admin-api.md` to `docs/admin-api.md` and remove or replace the old nested entry point.

## 2. Repair workspace metadata and local path conventions

- [x] 2.1 Update the root `Cargo.toml` workspace members to reference `bin/...` and `crates/...` paths instead of `aisix/...` paths.
- [x] 2.2 Update `.gitignore` so workspace build artifacts are ignored at `target/` rather than `aisix/target/`.
- [x] 2.3 Search active project files for `--manifest-path aisix/Cargo.toml`, `aisix/config`, `aisix/scripts`, and `aisix/target` references and rewrite them to the new root-level layout.

## 3. Update active scripts and contributor entry points

- [x] 3.1 Update `scripts/smoke-phase1.sh` so it works from the new root-level project layout without nested `aisix/...` assumptions.
- [x] 3.2 Update `README.md` to teach root-level `cargo build`, `cargo test`, `cargo clippy`, `cargo run -p aisix-gateway -- config/...`, `docker compose -f docker-compose.yml`, and `bash scripts/smoke-phase1.sh` commands.
- [x] 3.3 Update `AGENTS.md` to describe the repository root as the workspace root and to remove obsolete `aisix/...` path guidance.
- [x] 3.4 Update active architecture or admin documentation sections that still describe `aisix/` as the Rust workspace root.

## 4. Update CI and automation

- [x] 4.1 Update `.github/workflows/ci.yml` cache keys and cache paths to use root-level `Cargo.lock` and `target/`.
- [x] 4.2 Update `.github/workflows/ci.yml` build, fmt, clippy, test, gateway startup, config path, and smoke-test commands to run from the repository root without `--manifest-path aisix/Cargo.toml`.
- [x] 4.3 Re-scan workflow and shell files to confirm no active automation still depends on `aisix/`-prefixed Rust project paths.

## 5. Verify the migrated layout

- [x] 5.1 Confirm the repository root now contains the active Rust workspace directories and manifests expected by the new layout.
- [x] 5.2 Run `cargo build` from the repository root successfully.
- [x] 5.3 Run `cargo test` from the repository root successfully.
- [x] 5.4 Run `cargo clippy --all-targets -- -D warnings` from the repository root successfully.
- [x] 5.5 If dependencies are available, run the smoke flow from the repository root using `docker compose -f docker-compose.yml up -d redis etcd` and `bash scripts/smoke-phase1.sh`.

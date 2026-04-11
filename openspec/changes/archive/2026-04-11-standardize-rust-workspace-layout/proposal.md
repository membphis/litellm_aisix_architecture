## Why

The repository currently hides the Rust workspace under `aisix/`, while the repository root holds docs and project metadata. That makes the project feel unlike a typical Rust open source repository and forces contributors, CI, and docs to use awkward `aisix/...` and `--manifest-path aisix/Cargo.toml` paths everywhere.

## What Changes

- Move the Cargo workspace root from `aisix/` to the repository root.
- Promote `bin/`, `crates/`, `config/`, `scripts/`, `docker-compose.yml`, `Cargo.toml`, `Cargo.lock`, and the primary `README.md` to the repository root.
- Update active developer documentation, AGENTS guidance, CI workflows, and smoke-test commands to use root-level Rust project paths.
- Keep non-runtime project assets such as `docs/`, `openspec/`, `.github/`, and `.agents/` at the repository root.
- Avoid broad historical-document rewrites; only update paths in current entry-point docs and automation that must remain executable.

## Capabilities

### New Capabilities
- `repository-layout`: Defines the repository-level workspace layout, active path conventions, and contributor-facing command entry points for AISIX.

### Modified Capabilities

None.

## Impact

- Affected code and project files: Cargo workspace manifests, root ignore rules, CI workflow files, smoke scripts, README/AGENTS docs, and active documentation references.
- Affected developer workflows: local build/test/lint commands, gateway startup commands, docker compose commands, and CI cache/build paths.
- No runtime API behavior changes are intended; the change is focused on repository structure and contributor ergonomics.

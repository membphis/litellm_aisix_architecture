## Context

AISIX currently uses a split repository structure: the repository root contains top-level docs, OpenSpec artifacts, and project metadata, while the actual Rust workspace lives under `aisix/`. That layout was workable during early design-heavy development, but it now adds friction everywhere contributors interact with the project: Cargo commands require `--manifest-path aisix/Cargo.toml`, CI caches and build commands reference `aisix/target`, and active docs teach root-relative commands with an extra path prefix.

This change is cross-cutting because it affects the Cargo workspace root, contributor-facing documentation, automation, and scripts at the same time. The goal is to make the repository feel like a normal Rust open source project without changing gateway runtime behavior.

## Goals / Non-Goals

**Goals:**
- Make the repository root the only Cargo workspace root.
- Place active Rust project directories (`bin/`, `crates/`, `config/`, `scripts/`) at the repository root.
- Update active automation and contributor entry points to use root-level paths.
- Preserve the distinction between runtime project files and non-runtime repository assets such as `docs/`, `openspec/`, `.github/`, and `.agents/`.
- Minimize churn by updating only active, executable, contributor-facing references.

**Non-Goals:**
- No runtime feature work, API changes, or behavioral changes in the gateway.
- No attempt to rewrite every historical design note, archived plan, or legacy path mention across the repository.
- No compatibility shim that keeps both the old `aisix/` workspace and the new root workspace alive long-term.

## Decisions

### Decision: Flatten the Rust workspace into the repository root

The repository root will become the single Cargo workspace root, with `Cargo.toml` and `Cargo.lock` moved to the top level and workspace members referenced as `bin/...` and `crates/...`.

Rationale:
- This matches contributor expectations for Rust OSS repositories.
- It removes the need for `--manifest-path aisix/Cargo.toml` from local commands and CI.
- It makes related root-level assets such as `.github/`, `docs/`, and `openspec/` feel colocated with the actual project rather than wrapping it.

Alternatives considered:
- Add a new root `Cargo.toml` that still points into `aisix/...`: lower risk, but it preserves the awkward physical layout and does not really solve the repository-shape problem.
- Keep `aisix/` and only rewrite docs: lowest effort, but contributor ergonomics and automation complexity remain largely unchanged.

### Decision: Move active Rust project directories instead of adding compatibility layers

The implementation will physically move `bin/`, `crates/`, `config/`, `scripts/`, `docker-compose.yml`, primary README content, and the workspace manifests to the repository root instead of layering path indirection on top.

Rationale:
- A single clear structure is easier to maintain than transitional indirection.
- Path compatibility layers would leak into docs, CI, and future contributor assumptions.
- The repository is still early enough that a direct cleanup is cheaper than carrying long-lived migration debt.

Alternatives considered:
- Symlinks or duplicate manifests: introduces ambiguity and potential tool incompatibilities.
- Temporary dual-layout support: increases maintenance cost for little value because there are no published compatibility guarantees around repository-internal paths.

### Decision: Update only active path references, not all historical documents

The migration will update files that contributors execute or rely on as current truth: README, AGENTS guidance, CI workflows, smoke scripts, current admin API docs, and any architecture doc sections that describe the active workspace layout. Historical design and plan documents under `docs/superpowers/` may keep old path references unless they are still used as live setup instructions.

Rationale:
- Searching the repository shows hundreds of `aisix/` references, many inside historical notes and old implementation plans.
- Rewriting them all would add high noise and low value.
- Restricting edits to active entry points keeps the change reviewable.

Alternatives considered:
- Full repository-wide path rewrite: comprehensive, but creates unnecessary churn and obscures the meaningful structural changes.

### Decision: Keep root-level non-runtime project assets where they are

Directories such as `docs/`, `openspec/`, `.github/`, and `.agents/` will remain at the repository root. The restructure is about making the Rust project standard, not about collapsing all repository content into the Cargo workspace.

Rationale:
- These directories are already conventional root-level repository assets.
- Moving them would not improve Rust contributor ergonomics.
- Keeping them stable reduces collateral edits.

## Risks / Trade-offs

- [Missed active path reference] -> Use targeted searches for `aisix/`, `--manifest-path aisix/Cargo.toml`, `aisix/target`, and `aisix/config` across docs, scripts, and workflows before finishing.
- [Large file move obscures semantic edits] -> Separate structural moves from path-fix edits as much as possible, and verify the final tree explicitly.
- [Historical docs become visually inconsistent] -> Accept this trade-off and treat historical documents as snapshots unless they are still part of the active contributor workflow.
- [CI or local commands silently keep depending on the old layout] -> Verify from the repository root with `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, and smoke-related commands after migration.

## Migration Plan

1. Move the Rust workspace manifests and active project directories from `aisix/` to the repository root.
2. Update workspace member paths and ignore rules for the new root layout.
3. Update active scripts, docs, and AGENTS instructions to use root-level paths.
4. Update CI cache keys, build commands, binary paths, and smoke-test invocation.
5. Verify the repository root is now the canonical entry point for build, test, lint, and smoke flows.

Rollback strategy:
- If the migration fails mid-change, revert the working tree before merge rather than attempting to support both layouts simultaneously.
- Because this change is repository-structure only, rollback is a git revert of the structural commit set rather than a runtime deployment procedure.

## Open Questions

- Whether `docs/admin-api.md` should fully replace `aisix/docs/admin-api.md` during the same change or leave a short redirect stub is an implementation choice, but the active documentation entry point must end up at the repository root.
- Whether `docs/architecture.md` should be updated narrowly for workspace-path mentions or more broadly refreshed for current repository conventions can be decided during implementation as long as active instructions are accurate.

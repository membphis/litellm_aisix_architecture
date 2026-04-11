## ADDED Requirements

### Requirement: Repository root is the canonical Cargo workspace root
The repository SHALL expose a single Cargo workspace rooted at the repository root. Contributor-facing build, test, lint, and run commands MUST work from the repository root without requiring `--manifest-path aisix/Cargo.toml` or an equivalent subdirectory manifest override.

#### Scenario: Build commands run from repository root
- **WHEN** a contributor runs Cargo workspace commands from the repository root
- **THEN** the commands use the repository-root `Cargo.toml` as the canonical workspace manifest

#### Scenario: Workspace members use root-level paths
- **WHEN** Cargo resolves workspace members
- **THEN** each active AISIX package is addressed through root-level `bin/...` or `crates/...` paths rather than `aisix/...` paths

### Requirement: Active project directories use standard root-level Rust OSS layout
The repository SHALL place active Rust project directories and root-level operational assets at the repository root, including `bin/`, `crates/`, `config/`, `scripts/`, `Cargo.toml`, `Cargo.lock`, and the primary contributor `README.md`.

#### Scenario: Contributor inspects repository root
- **WHEN** a contributor lists the repository root contents
- **THEN** the active Rust workspace directories and primary manifests appear directly at the root alongside repository metadata directories such as `docs/` and `.github/`

#### Scenario: Ignore rules match the root workspace
- **WHEN** build artifacts are produced by the root workspace
- **THEN** repository ignore rules target root-level outputs such as `target/` instead of `aisix/target/`

### Requirement: Active automation and documentation use root-level paths
The repository SHALL ensure that active automation, scripts, and contributor-facing documentation reference the root-level workspace layout. CI workflows, smoke-test commands, setup commands, and active developer guidance MUST NOT require `aisix/`-prefixed paths for the Rust project.

#### Scenario: CI builds from repository root
- **WHEN** the CI workflow runs formatting, lint, test, and smoke build steps
- **THEN** those steps invoke Cargo and project files through root-level paths and cache root-level workspace outputs

#### Scenario: Contributor follows current documentation
- **WHEN** a contributor copies commands from the current README, AGENTS guidance, or active API documentation
- **THEN** the commands execute against root-level paths such as `config/...`, `scripts/...`, and `docker-compose.yml`

### Requirement: Historical documents are not mass-rewritten during layout migration
The repository migration SHALL update active, executable, contributor-facing path references, but it MUST NOT require a repository-wide rewrite of historical design notes, archived plans, or legacy implementation records solely to replace `aisix/` path prefixes.

#### Scenario: Historical notes retain old path references
- **WHEN** a historical design or plan document under an archival or note-taking area still refers to `aisix/...`
- **THEN** the migration can leave that reference untouched as long as active contributor entry points and automation have been updated

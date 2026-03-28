# Development Workflow

## Tooling

- Rust stable toolchain
- Docker and Docker Compose for local Qdrant stack
- just (optional convenience task runner)

## Common Local Commands

With cargo:

- cargo fmt
- cargo check
- cargo clippy --all-targets --all-features
- cargo test

With just:

- just dev
- just release
- just check
- just lint
- just test
- just docker-up
- just docker-down
- just docker-reset

## Recommended Inner Loop

1. Make focused code change.
2. Run cargo fmt.
3. Run cargo check.
4. Run cargo clippy --all-targets --all-features.
5. Run targeted tests, then cargo test.
6. Update docs if behavior changed.

## Running Locally

### Native

1. Start Qdrant.
2. Ensure models are present in models directory.
3. Run ingest.
4. Run search and cluster commands against the same Qdrant URL.

### Docker Compose

- docker compose up --build -d
- roobu container runs ingest by default.
- qdrant data persists in qdrant_data volume.
- checkpoint persists in roobu_data volume.

## Logging

- Default: roobu=info
- Increase detail with RUST_LOG, for example:
  - RUST_LOG=roobu=debug

## Testing Guidance

Current tests are primarily unit tests around:

- parsing and mapping behavior in site modules
- CLI argument parsing
- all-sites client composition
- point id encode/decode safety
- checkpoint roundtrip

When adding features:

- keep tests close to the changed module
- add focused tests for edge cases and fallback behavior
- avoid adding brittle tests tied to live external APIs

## Code Quality Expectations

- No compiler warnings.
- No clippy warnings for standard project checks.
- Keep module responsibilities focused.
- Prefer explicit error messages and context.
- Keep external API parsing resilient to optional/missing fields.

## Documentation Expectations

Update docs in docs when behavior changes, especially for:

- command flags and defaults
- supported site list
- ingest or scoring semantics
- deployment/runbook workflows

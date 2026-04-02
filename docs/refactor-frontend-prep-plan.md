# Refactor + Frontend Preparation Dossier

Date: 2026-04-02
Project: Roobu

## 1) Session Decisions and Hard Constraints

This section captures explicit decisions from this planning session so implementation can proceed without re-litigating scope.

- Frontend target: Next.js/React static export in `frontend/` (Bun, Tailwind v4, shadcn), embedded and served by the Rust binary.
- Backend/API prep: Yes, include full API prep for frontend.
- Realtime: WebSocket in phase 1 (not polling-only).
- Refactor cadence: Phased milestones (not big-bang rewrite).
- Site adapters: Keep all existing sites first-class.
- Testing bar: Unit + integration tests for commands/store; strict phase gates.
- Error handling direction: Strong typed errors in core layers; keep `anyhow` mainly at boundaries.
- Configuration direction: Keep CLI + env only (no config file system for now).
- Command UX: Keep the current CLI output style as much as possible.
- Public surface stability:
  - Command and checkpoint behavior must stay stable externally.
  - Internal breakage for clarity is allowed.
- Database constraint:
  - No destructive DB reset/reindex workflow.
  - Additive payload/schema evolution allowed if backward-compatible.
- Feature policy: Feature-flagged deactivation proposals are allowed.
- Migration notes: Required for high-risk changes.
- Documentation must stay in sync: `README.md`, `docs/architecture.md`, `docs/commands.md`, `docs/configuration.md`, plus ADR notes.

## 2) Baseline Audit Snapshot

Current quality gates are green:

- `cargo check`: pass
- `cargo clippy`: pass
- `cargo test`: pass (56/56)

Implication: this is mostly an architecture/maintainability refactor, not a broken-build rescue.

## 3) High-Priority Findings (What Is Weird or Fragile)

## 3.1 Runtime Safety and Correctness Risks

1. Panic path in runtime ingest code.
   - `src/ingest.rs` uses `sem.acquire().await.unwrap()`.
   - If semaphore is closed or task cancellation edge-cases happen, runtime panic is possible.
   - Rewrite target: return structured error and continue gracefully.

2. Startup panic path in tracing setup.
   - `src/main.rs` parses default tracing filter with `.unwrap()`.
   - Low-probability panic but avoidable in boundary code.

3. Unsafe thread-safety assertion around ONNX sessions.
   - `src/embed.rs` has `unsafe impl Send` and `unsafe impl Sync` for `Embedder`.
   - This is a red-flag hotspot in long-term maintainability and soundness.
   - Rewrite to remove unsafe marker traits by construction (safe concurrency boundary).

4. Checkpoint write semantics may not be truly atomic cross-platform.
   - `src/checkpoint.rs` saves temp file then `std::fs::rename(tmp, target)`.
   - On Windows, rename over existing file can fail.
   - This can cause intermittent checkpoint update failures in real deployments.
   - Constraint: must keep checkpoint format/semantics exactly.

5. Checkpoint load swallows corruption silently.
   - `src/checkpoint.rs` uses `serde_json::from_str(...).unwrap_or_default()`.
   - A malformed checkpoint becomes empty state without explicit warning.
   - This can trigger unintended reprocessing behavior.

## 3.2 Architecture and Code Organization Smells

1. `src/sites/mod.rs` is a monolith with too many responsibilities.
   - Data model (`Post`), client factory, URL mapping, trait dispatch, image validation, utility tests all mixed.

2. Adapter duplication is very high across site files.
   - Repeated HTTP client builder + retry/backoff pattern in nearly every `src/sites/*.rs` adapter.
   - Similar post mapping shape duplicated in multiple booru adapters.
   - E621 and E6AI are near-copy modules.

3. Command modules mix orchestration, domain logic, and presentation.
   - `src/commands/search.rs`, `src/commands/cluster.rs`, `src/commands/ingest.rs` combine business logic with terminal formatting.
   - This blocks easy API reuse for frontend.

4. Core pipeline module size is too large.
   - `src/ingest.rs` is large and combines cycle orchestration, queue topology, embed batching, checkpoint updates, and UI output.

5. Store layer does too much.
   - `src/store.rs` mixes collection lifecycle, payload schema definition, query decoding, clustering vector extraction, and stats traversal.

6. Custom graph-hdbscan implementation is very large and isolated.
   - `src/commands/graph_hdbscan.rs` is large and algorithm-heavy inside command folder.
   - This should move to dedicated domain layer with clearer ownership.

7. File size hotspots indicate weak module boundaries.
   - Largest files:
     - `src/commands/graph_hdbscan.rs`
     - `src/sites/civitai.rs`
     - `src/sites/mod.rs`
     - `src/ingest.rs`
     - `src/store.rs`

## 3.3 Configuration and Dependency Hygiene

1. Config constants are global and flat.
   - `src/config.rs` is a constant bag; needs typed config groups by concern.

2. Likely stale dependency.
   - `Cargo.toml` includes external `hdbscan` crate, but clustering uses local `graph_hdbscan` module.
   - Candidate for removal after verification.

3. Hidden/implicit cluster defaults are split.
   - Some cluster tuning knobs are exposed via CLI, others are hardcoded constants.
   - Needs explicit policy: public knobs vs internal constants.

## 3.4 Documentation Drift and Inconsistency

1. Batch-size default mismatch.
   - Code default: `src/config.rs` has `DEFAULT_BATCH_SIZE = 8`.
   - Docs claim 32 in both:
     - `README.md`
     - `docs/configuration.md`

2. Docs mention non-existent cluster flags.
   - `docs/troubleshooting.md` references options such as `--max-cluster-size`, `--projection-nnz`, `--min-samples` that are not current CLI flags.
   - `docs/configuration.md` cluster tuning guidance also references knobs not exposed in CLI.

3. Docs quality intent is good, but no enforced sync mechanism.
   - There is no explicit “docs drift checklist” gate tied to behavior changes.

## 4) Function and Module Rewrite Backlog

Priority order is based on frontend readiness + maintainability impact.

## Priority A (must do before frontend implementation)

1. `src/ingest.rs`
   - Rewrite into smaller modules:
     - scheduler/orchestration
     - download stage
     - embedding stage
     - persistence stage
     - cycle stats/reporting
   - Remove panic path and strengthen cancellation behavior.

2. `src/sites/mod.rs`
   - Split into:
     - site domain model (`Post`, site metadata)
     - client registry/factory
     - validation utilities
     - URL resolver
   - Reduce giant enum-dispatch boilerplate.

3. `src/embed.rs`
   - Remove unsafe Send/Sync strategy.
   - Encapsulate ONNX session execution behind safe worker abstraction.

4. `src/store.rs`
   - Separate concerns:
     - collection bootstrap/migration policy
     - write model mapping
     - query read model mapping
     - stats and scroll traversal

5. `src/checkpoint.rs`
   - Keep same format/semantics.
   - Improve write strategy and parse-failure diagnostics.

## Priority B (frontend preparation and API reuse)

1. `src/commands/search.rs`
   - Extract pure search application service (input -> result DTO), keep CLI formatter thin.

2. `src/commands/ingest.rs`
   - Extract all-sites client builder policy and credentials validation into dedicated module.

3. `src/commands/cluster.rs` + `src/commands/graph_hdbscan.rs`
   - Move clustering domain logic out of command module tree.
   - Keep command as adapter layer.

4. Site adapters:
   - Consolidate shared retry/backoff/http-construction utilities.
   - Create reusable booru-json mapping helpers.

## Priority C (cleanup and maintainability polish)

1. `src/config.rs`
   - Group constants into typed config structs.
   - Clarify public/user-facing defaults vs internal algorithm constants.

2. `src/ui.rs`
   - Keep visual style, but move to reusable renderer interfaces.
   - Prepare compatibility for CLI + WebSocket progress events.

3. `src/error.rs`
   - Expand domain-specific error enums by layer.
   - Restrict `anyhow` to top-level command/web handlers.

## 5) Proposed Target Structure (Phased, No DB Reset)

Target direction (high-level): split into reusable core + command/web adapters.

Suggested repository layout:

- `frontend/`
  - Next.js static export project.
- `crates/roobu-core/`
  - Domain and application logic (ingest, embed orchestration, store abstractions, site abstractions).
- `crates/roobu-cli/`
  - CLI argument parsing and terminal rendering, calls `roobu-core` services.
- `crates/roobu-web/`
  - HTTP + WebSocket API adapter layer for frontend.
- `src/main.rs` (temporary bridge)
  - During migration, can dispatch into CLI/web bins as compatibility shim until full split is complete.

If workspace split feels too disruptive initially, execute as two-step:

1. Internal module split within current crate.
2. Promote stable internal boundaries into separate crates.

## 6) Database Compatibility Strategy (Non-Destructive)

Non-negotiable: existing collection/data must keep working.

Rules:

1. Keep collection name and existing payload keys readable.
2. Keep point-id encoding behavior unchanged.
3. Keep checkpoint semantics unchanged.
4. Only additive payload evolution.

Recommended additive fields for frontend readiness (optional, staged):

- `source_payload_json` (compressed/trimmed per-site raw metadata, optional)
- `ingest_status` (if useful for operational UI)
- `content_type` (image/static metadata where available)
- `site_post_title` (when source has one)

Do not remove or rename existing payload fields until a compatibility window is complete.

## 7) Frontend API Preparation Plan

Style: REST + JSON + pagination, plus WebSocket realtime channel.

Phase-1 API surface (proposed):

1. Search endpoints
   - text/image/hybrid query endpoint matching existing semantics.
   - site filter support.

2. Ingest observability endpoints
   - current per-site status.
   - latest checkpoint state.

3. Stats endpoints
   - per-site distribution.
   - total indexed count.

4. Cluster endpoints
   - trigger + retrieve latest cluster summaries.

5. WebSocket channels
   - ingest cycle progress events.
   - per-site warning/error events.
   - optional search diagnostics stream.

Compatibility note:

- Keep command behavior stable by having CLI call the same core services used by web handlers.

## 8) Feature Reconsideration (Flagged, Not Removed)

Create optional feature flags to reduce complexity during stabilization:

1. `feature = "cluster"`
   - Allows focusing on ingest/search/frontend without deleting clustering.

2. `feature = "all-sites"` policy hardening
   - Keep behavior, but enable phased strictness and diagnostics.

3. `feature = "raw-site-metadata"`
   - Gate additive payload-heavy metadata fields.

No destructive DB changes should be tied to these flags.

## 9) Documentation Sync Backlog

Immediate doc fixes needed:

1. Update batch-size defaults in:
   - `README.md`
   - `docs/configuration.md`

2. Remove or correct non-existent cluster flags in:
   - `docs/troubleshooting.md`
   - `docs/configuration.md`

3. Add ADR folder and first ADRs:
   - ADR: crate split and module boundaries.
   - ADR: DB compatibility and additive payload policy.
   - ADR: API + WebSocket contract strategy.

4. Add explicit docs-sync checklist item in development workflow and PR template.

## 10) Implementation Roadmap

## Milestone 0: Safety and Guardrails

- Remove panic paths in runtime code.
- Harden checkpoint write/load behavior with explicit diagnostics.
- Add tests for checkpoint corruption + write replacement behavior.

Exit criteria:

- no runtime unwrap/expect in production paths
- compatibility behavior unchanged externally

## Milestone 1: Core Boundary Extraction

- Move ingest/store/embed/search business logic into domain/application modules.
- Keep CLI output style unchanged.
- Add integration tests for command-to-core behavior equivalence.

Exit criteria:

- CLI outputs remain stylistically consistent
- command behavior parity confirmed

## Milestone 2: Site Layer Normalization

- Introduce shared HTTP/retry utilities.
- Deduplicate booru adapter patterns.
- Split `sites/mod.rs` into focused files.

Exit criteria:

- all site adapters still first-class
- no behavior regressions in existing site tests

## Milestone 3: Web/API Entry Surface

- Add REST endpoints for search/stats/status.
- Add WebSocket channel for ingest progress.
- Keep CLI as supported external interface.

Exit criteria:

- frontend can query/search and receive realtime ingest updates
- no DB reset required

## Milestone 4: Frontend Integration

- Add `frontend/` static export pipeline.
- Embed static assets in binary serving path.
- Wire frontend to REST + WebSocket endpoints.

Exit criteria:

- single-binary deploy still works
- frontend usable without separate node runtime in production

## Milestone 5: Final Cleanup and Docs Convergence

- Remove stale code/dependency/config leftovers.
- Ensure docs fully aligned.
- Publish migration notes for high-risk changes.

Exit criteria:

- `cargo fmt`, `cargo clippy -D warnings`, `cargo test` pass
- docs synchronized with runtime behavior

## 11) Testing Strategy for Refactor Safety

Mandatory for each milestone:

1. Unit tests for transformed modules.
2. Integration tests for command compatibility.
3. Golden-style tests for key CLI output sections (style preservation).
4. Regression tests for checkpoint and point-id compatibility.
5. Store compatibility tests against existing payload assumptions.

Recommended additional tests for 150K+ embeddings reality:

1. Ingest throughput smoke benchmark.
2. Search latency benchmark with representative corpus.
3. Memory profile checks for embedding batch stages.

## 12) Handoff Notes for Implementation Agent

When executing this plan:

1. Preserve external behavior of commands/checkpoint/database.
2. Prefer internal simplification over cosmetic churn.
3. Keep changes incremental and merge-safe.
4. Update docs in same PR as behavior-affecting changes.
5. For high-risk changes, include migration note section in PR.
6. If a proposed cleanup conflicts with DB compatibility, DB compatibility wins.

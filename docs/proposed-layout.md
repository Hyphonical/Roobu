# Proposed Codebase Layout

## Current Issues

The current flat `src/` layout with a `commands/` and `sites/` subdirectory works but has grown unwieldy as features have been added. The main problems:

1. **Flat top-level modules** — `src/` has 10+ files at the root level with no grouping by concern
2. **Mixed responsibilities** — `ingest.rs` contains pipeline logic while `commands/ingest.rs` contains CLI wiring, but they're in different places
3. **No clear separation** between application services (reusable logic) and presentation layers (CLI, web)
4. **Site adapters** are all in one `sites/` directory but some are much larger than others

## Proposed Layout

```
src/
├── main.rs                      # Entry point, tracing init, CLI dispatch
├── error.rs                     # RoobuError enum
├── config.rs                    # Constants and defaults
│
├── cli/                         # CLI argument definitions
│   ├── mod.rs                   # Cli struct, Commands enum
│   └── args.rs                  # Per-command argument structs (if needed)
│
├── embed/                       # ONNX embedding logic
│   ├── mod.rs                   # Embedder struct, EMBED_DIM constant
│   ├── model.rs                 # Session creation, optimization fallback
│   ├── preprocess.rs            # Image preprocessing, tensor conversion
│   └── blend.rs                 # Text+image embedding blending
│
├── store/                       # Qdrant database client
│   ├── mod.rs                   # Store struct, collection management
│   ├── schema.rs                # Point ID encoding, payload helpers
│   ├── search.rs                # Search queries
│   ├── cluster.rs               # Vector fetching for clustering
│   └── stats.rs                 # Site distribution statistics
│
├── ingest/                      # Ingest pipeline
│   ├── mod.rs                   # IngestConfig, run(), run_multi()
│   ├── pipeline.rs              # Download → Embed → Upsert pipeline
│   ├── checkpoint.rs            # Checkpoint load/save/get/set
│   └── cycle.rs                 # Single-site cycle logic, stats
│
├── sites/                       # Booru site adapters
│   ├── mod.rs                   # Post, SiteKind, SiteClient, BooruClient trait
│   ├── common.rs                # URL normalization helpers
│   ├── http_client.rs           # Shared HTTP client, retry logic
│   ├── validate.rs              # Image validation (validate_downloaded_image)
│   │
│   ├── rule34.rs                # Rule34 adapter (requires credentials)
│   ├── gelbooru.rs              # Gelbooru adapter (requires credentials)
│   ├── e621.rs                  # e621 adapter (optional credentials)
│   ├── kemono.rs                # Kemono adapter (optional session)
│   │
│   ├── danbooru.rs              # Danbooru-style adapters (no credentials)
│   ├── aibooru.rs
│   ├── civitai.rs
│   ├── e6ai.rs
│   ├── konachan.rs
│   ├── safebooru.rs
│   ├── xbooru.rs
│   └── yandere.rs
│
├── commands/                    # CLI command handlers
│   ├── mod.rs                   # Dispatch function
│   ├── ingest.rs                # CLI adapter → ingest::run()
│   ├── search.rs                # CLI adapter → search logic
│   ├── cluster.rs               # CLI adapter → clustering
│   ├── stats.rs                 # CLI adapter → statistics
│   └── serve.rs                 # CLI adapter → web server
│
├── web/                         # Web API server
│   ├── mod.rs                   # Router creation
│   ├── state.rs                 # AppState, IngestStatus
│   ├── handlers.rs              # REST endpoint handlers
│   └── ws.rs                    # WebSocket progress streaming
│
├── cluster/                     # Clustering algorithms
│   ├── mod.rs                   # ClusterSummary, build_cluster_input
│   └── graph_hdbscan.rs         # GraphHDBSCAN implementation
│
└── ui/                          # Terminal output utilities
    ├── mod.rs                   # Public functions (header, step, etc.)
    └── macros.rs                # ui_step!, ui_detail!, etc.
```

## Key Improvements

### 1. Group by Concern, Not Type
- `embed/` groups all embedding logic together (model loading, preprocessing, blending)
- `store/` groups all Qdrant operations together (search, cluster fetch, stats)
- `ingest/` groups the full pipeline (checkpoint, cycle, pipeline stages)

### 2. Clear Service/Presentation Boundary
- `ingest/`, `embed/`, `store/` contain **reusable application services**
- `commands/` contains **CLI-specific adapters** that call services and format terminal output
- `web/` contains **HTTP/WebSocket handlers** that call the same services

### 3. Site Adapter Organization
- Group sites by credential requirements (makes it clear which need setup)
- Shared utilities (`http_client`, `common`, `validate`) stay in `sites/` root
- Each adapter file is focused on a single site

### 4. Smaller, Focused Modules
- `embed/model.rs` — only session creation and optimization
- `embed/preprocess.rs` — only image preprocessing and tensor conversion
- `store/schema.rs` — only point ID encoding and payload extraction
- `ingest/checkpoint.rs` — only checkpoint persistence

### 5. UI Module Split
- Separate macros from functions for clarity
- Easier to add new output formatters (JSON, structured logs) later

## Migration Strategy

This is a **non-breaking reorganization**. The public API (CLI commands, web endpoints, config) remains identical. The changes are purely internal:

1. Create new directory structure
2. Move code into appropriate modules
3. Update `mod.rs` files and import paths
4. Run `cargo test` to verify nothing broke
5. Run `cargo clippy` to catch any issues

## Alternative: Keep Current Layout with Improvements

If a full reorganization is too disruptive, the current layout can be improved with:

1. **Add module-level documentation** to every file (done ✓)
2. **Group related files** with comment headers in `main.rs`
3. **Extract large functions** into smaller, well-named helpers
4. **Add a `PLAN.md`** before any future major changes

The cleanup work already done (documentation, consistent style, dead code removal) applies to either layout.

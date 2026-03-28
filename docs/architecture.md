# Architecture

## High-Level Overview

Roobu is a self-hosted semantic retrieval system for booru-style image sources.

Core stages:

1. Fetch recent posts from one or more site APIs.
2. Download preview images and validate them.
3. Generate two embeddings per post:
   - image embedding from preview pixels
   - tags embedding from normalized text tags
4. Upsert vectors and metadata into Qdrant.
5. Query Qdrant using text, image, or hybrid search.
6. Optionally run HDBSCAN over image vectors for cluster discovery.

## Runtime Components

- CLI entrypoint
  - Parses command-line arguments and dispatches to command handlers.
- Ingest pipeline
  - Runs continuously with checkpoint resume and per-site fault isolation.
- Embedding engine
  - Uses ONNX Runtime sessions for SigLIP2 text and vision models.
- Vector store
  - Manages Qdrant collection lifecycle, upsert, search, and vector scrolling.
- Site adapters
  - One adapter per source site implementing a shared client trait.

## Module Map

- src/main.rs
  - Initializes logging and executes command dispatch.
- src/commands
  - Ingest/search/cluster orchestration and argument validation.
- src/ingest.rs
  - Core continuous ingest loop, batching, downloads, embedding, upsert, checkpoint updates.
- src/embed.rs
  - Model loading, image preprocess, text tokenization, embedding, and blending.
- src/store.rs
  - Qdrant collection management, named vectors, weighted search merge, cluster vector fetch.
- src/sites
  - Site-specific HTTP/API adapters plus shared post model and validation logic.
- src/checkpoint.rs
  - JSON checkpoint persistence by site name.
- src/config.rs
  - Runtime defaults and constants used across commands.

## Data Model

Each ingested post carries:

- post id
- site identifier and site namespace id
- preview URL (used for download)
- tags string
- rating
- optional canonical post URL

At upsert time, each point stores:

- point id: encoded from site namespace and post id
- named vectors:
  - image: 1024-d float vector
  - tags: 1024-d float vector
- payload:
  - post_id
  - site
  - post_url
  - rating

## Point ID Strategy

Roobu prevents cross-site id collisions by encoding point ids as:

point_id = site_namespace * POINT_ID_SITE_MULTIPLIER + post_id

This allows the same post id number to exist across multiple sites without collisions.

## Ingest Flow

### Single-site Mode

- Build one site client.
- Load checkpoint for that site.
- Repeat forever:
  - fetch recent posts newer than checkpoint
  - preflight filter (has preview URL, aspect ratio gate)
  - download previews with bounded concurrency
  - decode/size/aspect validation
  - batch embed image and tags vectors
  - upsert to Qdrant
  - advance checkpoint to max post id observed
  - sleep poll interval

### All-sites Mode

- Build a sequence of clients (some optional based on credentials).
- Iterate sites sequentially in a loop.
- Per site:
  - run one ingest cycle
  - log and continue if the cycle fails
  - sleep based on per-site cadence target

This design prioritizes stability and predictable resource use over maximum ingest throughput.

## Image Validation Gates

Two layers protect embedding quality and runtime:

1. Preflight checks before download:
   - preview URL must be non-empty
   - declared aspect ratio must be within configured max
2. Post-download checks:
   - minimum byte size
   - image decode must succeed
   - minimum edge size
   - measured aspect ratio must be within configured max

## Search Flow

1. Validate query mode:
   - text only
   - image only
   - text + image hybrid
2. Load only required model components (text, vision, or both).
3. Build query embedding:
   - text only: text vector
   - image only: image vector
   - hybrid: normalized weighted blend of text and image vectors
4. Query both named vectors in Qdrant (image and tags) when relevant.
5. Merge scores per point id with weighted sum.
6. Sort descending and return top results.

## Cluster Flow

1. Scroll image vectors from Qdrant, optionally site-filtered.
2. Optionally project vectors to lower dimensions via sparse random projection.
3. Run HDBSCAN with user-provided hyperparameters.
4. Summarize cluster cohesion and representative sample URLs.
5. Print ranked preview members per cluster.

## Storage Strategy

Roobu uses one Qdrant collection with named vectors.

- Distance metric: cosine
- Quantization: scalar int8, quantile 0.99
- Vector storage: on disk

This balances memory footprint and retrieval quality for self-hosted setups.

## Fault Tolerance Model

- Site fetch timeout protects cycle progress.
- Per-site cycle errors do not crash all-sites mode.
- Checkpoint writes are atomic via temporary file + rename.
- Empty/invalid download batches are skipped without aborting the ingest process.

## Concurrency Model

- Download concurrency is bounded by a semaphore.
- Embedding is done in blocking worker threads to avoid stalling async IO.
- Dual-vector search requests can execute concurrently when both vectors are used.

## Logging and Observability

- Tracing is initialized at startup.
- Default log filter is roobu=info.
- More detail can be enabled via RUST_LOG.
- Ingest emits cycle-level and batch-level status plus skip reasons.

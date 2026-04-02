# Configuration

## Core Defaults

Roobu defaults are defined in src/config.rs.

### Service and Paths

- Qdrant URL: http://localhost:6334
- Models directory: models
- Checkpoint path: checkpoint.json
- Qdrant collection: roobu

### Ingest Defaults

- Poll interval: 60 seconds
- Batch size: 8
- Download concurrency: 8
- Per-site fetch timeout: 20 seconds

### Search Defaults

- Result limit: 10
- Hybrid image-query weight: 1.0
- Fetch multiplier: 3 (internal oversampling before merge/truncate)

### Cluster Defaults

- Page size: 256
- Max points: 50000
- Min cluster size: 10
- Preview URLs per cluster: 10
- Epsilon: 0.05
- Low-cohesion threshold: 0.75
- Projection nnz: 2
- Projection seed: 1215765097

### Embedding and Image Validation

- Embedding dimension: 1536
- Image input size: 256 x 256
- Text sequence length: 64
- Minimum downloaded bytes: 500
- Minimum image edge: 32 px
- Maximum aspect ratio: 2.0

## Environment Variables

### Common

- QDRANT_URL
  - Used by ingest, search, and cluster commands when provided.
- ROOBU_ONNX_OPTIMIZATION
  - safe, balanced, or aggressive.
- RUST_LOG
  - Logging filter; default fallback is roobu=info.

### Site Credentials

- Rule34
  - RULE34_API_KEY
  - RULE34_USER_ID
- e621
  - E621_LOGIN
  - E621_API_KEY
- Gelbooru
  - GELBOORU_API_KEY
  - GELBOORU_USER_ID
- Kemono
  - KEMONO_SESSION
  - KEMONO_BASE_URL

## Models Layout

The models directory must include:

- vision_model_q4f16.onnx

Optional for text/hybrid search queries:

- text_model_q4f16.onnx
- tokenizer.json

If your ONNX export has sidecar data shards, keep them next to the corresponding model file.

## ONNX Optimization Levels

- safe
  - GraphOptimizationLevel::Level1
  - Best reliability for constrained hosts.
- balanced
  - GraphOptimizationLevel::Level2
- aggressive
  - GraphOptimizationLevel::All
  - Highest optimization; validate for your host and model build.

## Checkpoint Behavior

Checkpoint file is a JSON object mapping site name to last processed post id.

Characteristics:

- loaded at startup
- updated after successful upsert batches
- saved atomically via temporary file then rename

## Tuning Guidance

### Throughput Tuning

If ingest is too slow:

- Increase --download-concurrency in controlled steps.
- Increase --batch-size if CPU and memory headroom allow.
- Use balanced/aggressive ONNX optimization only after validation.

### Stability Tuning

If site endpoints are flaky:

- Increase --site-fetch-timeout-secs.
- Keep all-sites mode to isolate failures per site.

### Search Relevance Tuning

- Use --weight closer to 1.0 when image-query semantics should dominate hybrid queries.
- Use --weight closer to 0.0 when text-query semantics should dominate hybrid queries.

### Cluster Quality Tuning

- Raise --min-cluster-size for tighter clusters.
- Set --projection-dims for faster clustering on large corpora.
- Increase --max-points only when memory budget allows.

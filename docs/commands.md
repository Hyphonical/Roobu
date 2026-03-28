# Commands

This document explains operational behavior for all CLI commands.

## Global Notes

- All commands are exposed through the roobu binary.
- Qdrant URL defaults to http://localhost:6334.
- ONNX optimization profile defaults to safe.
- Use roobu <command> --help for the latest generated flag output.

## ingest

Continuous indexing loop that fetches posts, embeds them, and upserts vectors.

### Modes

- Single-site mode
  - Provide --site <site_name>.
- All-sites mode
  - Omit --site.
  - Sites run sequentially in a loop.

### Key Options

- --site
  - One of: rule34, e621, safebooru, xbooru, kemono, aibooru, danbooru, civitai, e6ai, gelbooru, konachan, yandere.
- --qdrant-url
  - Qdrant gRPC endpoint.
- --models-dir
  - Folder containing vision_model_q4f16.onnx, text_model_q4f16.onnx, tokenizer.json.
- --checkpoint
  - JSON file storing per-site last processed post id.
- --poll-interval
  - Seconds between ingest cycles.
- --batch-size
  - Number of posts processed per embed/upsert batch.
- --download-concurrency
  - Maximum concurrent preview downloads.
- --site-fetch-timeout-secs
  - Hard timeout per site fetch operation.

### Credential-related Options

- Rule34
  - --rule34-api-key
  - --rule34-user-id
- e621
  - --e621-login and --e621-api-key must be supplied together if used.
- Gelbooru
  - --gelbooru-api-key
  - --gelbooru-user-id
- Kemono
  - --kemono-session (optional)
  - --kemono-base-url (optional)

### All-sites Credential Behavior

- Rule34 and Gelbooru are included only if their required credential pairs are complete.
- Partial credential pairs for those sites cause an immediate error.
- Missing full pairs are treated as intentional skip in all-sites mode.

### What ingest Persists

- Qdrant points with image + tags vectors and payload metadata.
- Checkpoint file updates after successful upserts.

### Example

- Single site:
  - roobu ingest --site civitai --qdrant-url http://localhost:6334
- All sites:
  - roobu ingest --qdrant-url http://localhost:6334

## search

Semantic retrieval from Qdrant using text, image, or hybrid query vectors.

### Query Modes

- Text only
  - roobu search "red hair"
- Image only
  - roobu search --image ./query.png
- Hybrid text + image
  - roobu search --image ./query.png --weight 0.6 "red hair"

### Key Options

- query positional arg
  - Optional when --image is provided.
- --image
  - Path to image for visual query.
- --weight
  - Image weight in [0.0, 1.0]. Tags weight is computed as 1.0 - weight.
- --limit
  - Number of results to return.
- --site
  - Optional payload filter to one indexed site.

### Scoring Behavior

Roobu can query both named vectors and merge scores.

- image contribution = image_weight * image_similarity
- tags contribution = tags_weight * tags_similarity
- final score = sum of active contributions

### Output

Each result prints:

- decoded post id
- match percentage
- resolved post URL

## cluster

Runs HDBSCAN over stored image vectors, optionally with dimensionality reduction.

### Key Options

- --site
  - Restrict vector fetch to one site payload value.
- --page-size
  - Qdrant scroll page size.
- --max-points
  - Maximum number of vectors loaded for clustering.
- --min-cluster-size
  - Minimum samples to form a cluster.
- --min-samples
  - Optional neighborhood override.
- --max-cluster-size
  - Optional upper bound for cluster size stability.
- --epsilon
  - Strictness threshold.
- --allow-single-cluster
  - Allow one dominant cluster.
- --projection-dims
  - Optional lower dimension target before clustering.
- --projection-nnz
  - Sparse projection density per source dimension.
- --projection-seed
  - Deterministic seed for projection mapping.
- --limit
  - Sample URL count shown per cluster preview.

### Validation Rules

Cluster validates ranges for size, epsilon, dimensions, and projection settings before execution.

### Output Summary

- number of clusters
- total samples
- noise count and percentage
- per-cluster cohesion
- representative post URL
- preview member URLs

## Operational Guidance

- Start with ingest and let it run long enough to build a healthy index.
- Use search for retrieval and relevance checks.
- Use cluster for discovery, deduplication workflows, and corpus health inspection.

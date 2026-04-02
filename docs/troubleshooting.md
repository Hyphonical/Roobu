# Troubleshooting

## Build and Runtime

### ONNX model not found

Symptoms:

- startup fails when loading embedder
- model component not loaded errors

Checks:

- models directory exists
- required file is present:
  - vision_model_q4f16.onnx
- optional files for text/hybrid search are present when needed:
  - text_model_q4f16.onnx
  - tokenizer.json
- models-dir flag points to the correct path

### Qdrant connection failure

Symptoms:

- ingest/search/cluster fails during store initialization

Checks:

- Qdrant is running and reachable on gRPC URL
- URL matches expected endpoint (default http://localhost:6334)
- if using Docker Compose, confirm qdrant healthcheck is passing

### No search results

Checks:

- ingest has run long enough to index data
- checkpoint file is not stuck at very low values due to repeated cycle failures
- query mode is valid and not empty
- --site filter matches payload values exactly
- when using text+image hybrid queries, try lower image weight for text-heavy intent

### Ingest loops but indexes little data

Checks:

- preview URLs may be missing for many posts
- image validation filters may exclude content:
  - tiny files
  - too-small dimensions
  - extreme aspect ratio
- site endpoint may be rate-limited or timing out

Tuning ideas:

- increase --site-fetch-timeout-secs
- adjust --download-concurrency for network conditions
- inspect logs with RUST_LOG=roobu=debug

### Credential errors in all-sites mode

Behavior:

- Rule34 and Gelbooru require complete credential pairs.
- Partial pairs cause a hard error.
- Missing full pairs are treated as skip.

Fix:

- provide both required variables for a site, or remove both.

## Clustering Quality

### Too many noise points

Try:

- lower --min-cluster-size
- increase --max-points for denser sample coverage

### Clusters are too broad or mixed

Try:

- increase --min-cluster-size
- set --projection-dims to moderate dimensions if high-dimensional noise dominates

### Clustering is slow

Try:

- reduce --max-points
- set --projection-dims (for example 256)

## Checkpoint Issues

### Need to re-index from scratch

Options:

- delete checkpoint file and restart ingest
- reset Qdrant collection (or use docker-reset in compose workflows)

Note:

Deleting checkpoint alone does not delete existing vectors. Reset both checkpoint and collection for a true clean re-ingest.

## Site Adapter Debugging

If one site fails repeatedly:

1. Run single-site ingest with that adapter.
2. Enable debug logs.
3. Check for API schema drift in raw mapping structs.
4. Verify fallback URL logic still yields downloadable image bytes.
5. Add/update module tests that reproduce the failure payload.

## Recovery Principle

Roobu is designed to continue in the presence of partial failures. In all-sites mode, a single broken site should not stop ingestion for other sites.

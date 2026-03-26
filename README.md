# Roobu 🔍

**Semantic booru search that runs on your own machine.**

Find posts by what is in them, not what the uploader typed in five seconds at 3 AM. Roobu ingests booru posts, generates embeddings with SigLIP2, stores vectors in Qdrant, and gives you fast natural-language search from the CLI.

No SaaS. No API gateway maze. No mystery black box in someone else's cloud.

## Table of Contents

- [What's This About?](#whats-this-about)
- [Why Does This Exist?](#why-does-this-exist)
- [Features](#features)
- [Quick Start](#quick-start)
- [Docker](#docker)
- [Usage](#usage)
  - [`ingest` - Pull + index new posts](#ingest---pull--index-new-posts)
  - [`search` - Find matching posts](#search---find-matching-posts)
- [Qdrant Quantization](#qdrant-quantization)
- [Resetting the Database](#resetting-the-database)
- [Contributing](#contributing)
- [License](#license)

## What's This About?

Roobu is a self-hosted semantic image search tool for booru-style sites.

It continuously ingests new posts, embeds both thumbnails and tags into a shared vector space, then searches by cosine similarity.

That means queries like:

```bash
roobu search --qdrant-url http://localhost:6334 "red hair, black dress"
```

...can find relevant posts even when exact keywords are missing.

## Why Does This Exist?

Because text tags alone are brittle, exact-match search is annoying, and setting up giant infra for personal search feels silly.

Roobu is intentionally simple:

- Rust binary
- ONNX models on disk
- Qdrant for vector search
- One ingest command, one search command

It is built for "I want this running on my VPS tonight" energy.

## Features

- Natural-language search over booru content
- Hybrid scoring with two vectors per post:
  - `image` vector from thumbnail
  - `tags` vector from normalized tags text
- Query-time weighting (`--weight`) between image and tag relevance
- Continuous ingestion loop with checkpoint resume support
- Pre-flight and post-download image validation filters
- Qdrant named vectors with scalar quantization (`Int8`, quantile `0.99`)
- Site namespace-safe point IDs for multi-site growth
- Dockerfile + Docker Compose support

## Quick Start

### 1. Build

```bash
git clone https://github.com/Hyphonical/Roobu.git
cd roobu
cargo build --release
```

### 2. Add Models

Place model files in `models/`:

- `vision_model_q4f16.onnx`
- `text_model_q4f16.onnx`
- `tokenizer.json`

If your ONNX export includes a `.onnx_data` shard, keep it beside the matching `.onnx` file.

### 3. Start Qdrant

```bash
docker compose up -d qdrant
```

### 4. Ingest data

```bash
roobu ingest \
  --qdrant-url http://localhost:6334 \
  --api-key "$RULE34_API_KEY" \
  --user-id "$RULE34_USER_ID"
```

### 5. Search

```bash
roobu search --qdrant-url http://localhost:6334 "red hair, black dress"
```

## Docker

### Build image locally

```bash
docker build -t roobu:latest .
```

or with `just`:

```bash
just docker-build
```

### Run full stack (Qdrant + Roobu ingest)

```bash
docker compose up --build -d
```

or with `just`:

```bash
just docker-up
```

The Docker CI workflow in `.github/workflows/ci-docker.yml` builds the image on pushes and pull requests.

## Usage

### `ingest` - Pull + index new posts

```bash
roobu ingest [OPTIONS]

Options:
  --qdrant-url <URL>            Qdrant gRPC endpoint [default: http://localhost:6334]
  --models-dir <PATH>           Model directory [default: models]
  --checkpoint <PATH>           Checkpoint file [default: checkpoint.json]
  --poll-interval <SECONDS>     Poll interval [default: 60]
  --batch-size <N>              Batch size [default: 16]
  --download-concurrency <N>    Concurrent downloads [default: 8]
  --api-key <KEY>               Rule34 API key (or RULE34_API_KEY)
  --user-id <ID>                Rule34 user id (or RULE34_USER_ID)
```

Example:

```bash
roobu ingest \
  --qdrant-url http://localhost:6334 \
  --models-dir ./models \
  --checkpoint ./checkpoint.json \
  --poll-interval 45 \
  --batch-size 24 \
  --download-concurrency 12 \
  --api-key "$RULE34_API_KEY" \
  --user-id "$RULE34_USER_ID"
```

### `search` - Find matching posts

```bash
roobu search <QUERY> [OPTIONS]

Options:
  -l, --limit <N>               Results to return [default: 10]
  --qdrant-url <URL>            Qdrant gRPC endpoint [default: http://localhost:6334]
  --models-dir <PATH>           Model directory [default: models]
  --weight <0.0-1.0>            Image weight [default: 1.0]
  --site <NAME>                 Payload site filter (e.g. rule34)
```

Notes:

- `--weight` controls image-vector influence.
- Tag-vector weight is computed as `1.0 - weight`.

Examples:

```bash
# Fully visual
roobu search --qdrant-url http://localhost:6334 --weight 1.0 "woman on beach"

# Hybrid scoring (70% image, 30% tags)
roobu search --qdrant-url http://localhost:6334 --weight 0.7 "pink bedroom"

# Restrict to one site
roobu search --qdrant-url http://localhost:6334 --site rule34 "elf warrior"
```

## Qdrant Quantization

Roobu configures scalar quantization for both vectors when the collection is first created:

- Type: `Int8`
- Quantile: `0.99`
- `on_disk: true`
- `always_ram: false`

This reduces vector memory footprint while avoiding forced in-RAM storage of quantized data.

Important: this only applies at collection creation time. Existing collections keep their current config.

## Resetting the Database

If you want quantization settings to apply on an existing setup, delete the current collection/storage and let Roobu recreate it.

With Docker Compose:

```bash
docker compose down -v
docker compose up --build -d
```

or with `just`:

```bash
just docker-reset
just docker-up
```

`docker compose down -v` removes the Qdrant storage volume and the Roobu data volume.

## Contributing

Issues and PRs are welcome. Keep changes focused, run checks locally, and include clear reproduction steps for bug reports.

## License

MIT. See `LICENSE`.

---

Built with Rust
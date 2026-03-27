# Roobu — Project Plan

## Overview

Roobu is a self-hosted semantic image search engine for booru-style imageboards. It continuously ingests posts from Rule34 (and is designed to extend to other booru sites without schema migration), generates vector embeddings of both the post thumbnail and its tag set using a SigLIP2 ONNX model, stores the embeddings in a Qdrant vector database, and exposes a command-line search interface that accepts natural language queries.

The core idea: SigLIP2 is a contrastive vision-language model that maps images and text into the same 1024-dimensional vector space. Cosine similarity between a query vector and an image vector is semantically meaningful. Searching for "a red dragon breathing fire" finds posts whose thumbnails are visually close to that description, even if the tags do not literally say those words.

**A known limitation:** SigLIP2 was trained on general web data by Google. It was not trained on NSFW or booru-specific content. The model will underperform on that content compared to a domain-specific model. The tag-based hybrid vector partially compensates — tags encode the declared subject matter and will match natural language queries more reliably than the image vector alone in ambiguous cases. This is a known ceiling on retrieval quality that cannot be resolved without fine-tuning or replacing the backbone model. It is accepted for V1.

**Intended deployment:** A Linux VPS with no GPU (CPU-only inference). Vectors are stored on disk in Qdrant. The system is designed to run unattended.

---

## Legal Considerations

Roobu does not store any images. It downloads thumbnails transiently to generate embeddings, then discards them. What is stored is a 1024-element float32 vector per image, which is not reversible to the source image in any meaningful sense. Post metadata stored (URL, rating, post ID) is publicly available through the site's own API.

Rule34's API (`api.rule34.xxx`) is publicly documented, unauthenticated, and used by many third-party tools. Roobu identifies itself via a `User-Agent` header and applies rate limiting to avoid abusive request patterns.

---

## Technical Foundation

### SigLIP2 and Embedding Space

SigLIP2 (Sigmoid Loss for Language Image Pre-Training, second generation) is a vision-language model that aligns image and text representations in a shared 1024-dimensional vector space through contrastive training. Both modalities produce L2-normalized vectors. For matching image-text pairs, the dot product of their unit vectors (which equals cosine similarity) is high. For mismatched pairs it is low.

This means:
- A text query is encoded to a point in the same space as images.
- Searching is nearest-neighbor lookup: find the image vectors closest to the query vector.
- No labels, classifiers, or per-category training is needed.

### Model Selection

**Chosen model: `onnx-community/siglip2-large-patch16-256-ONNX`, Q4F16 quantization.**

This is a FixRes (fixed-resolution) model. All inputs are resized and center-cropped to exactly 256×256 before inference. This is critical — it means every image in a batch has an identical tensor shape, enabling true batched ONNX inference.

The five candidate models were evaluated as follows:

| Model | Why rejected or chosen |
|---|---|
| `siglip2-large-patch16-256` | **Chosen.** 16×16 = 256 patches. Best spatial resolution for small thumbnails. Acceptable CPU throughput. |
| `siglip2-large-patch32-256` | Patch32 produces 8×8 = 64 patches — a much coarser grid. At 250px thumbnails the image is already small; the coarser resolution loses meaningful detail. Speed gain does not justify quality loss. |
| `siglip2-so400m-patch16-256` | Same reasoning as large, more extreme. |
| `siglip2-giant-opt-patch16-256` | Unacceptable CPU throughput for bulk ingestion across multiple sites with large backlogs. |

**On hardware:** Development happens on a laptop with an 8GB VRAM NVIDIA GPU. Production runs on a VPS with no GPU (32GB RAM, AMD EPYC, CPU-only inference). The model must be fast on CPU. large-patch16 Q4F16 is the only candidate that meets throughput requirements for multi-site scale.

**Note on naflex:** The `siglip2-large-patch16-naflex` variant was evaluated and rejected. The ONNX export of the naflex vision encoder does not share an aligned embedding space with the available text encoder exports. Cosine similarity between text queries and naflex-encoded images is near-zero or negative regardless of semantic relevance. This is a known issue with the ONNX export and is not solvable at the preprocessing level.

### Model Files

Both the vision and text encoders come from the **same repository**: `onnx-community/siglip2-large-patch16-256-ONNX`. Using encoders from the same checkpoint is mandatory — they share an embedding space because they were trained jointly.

| File | Purpose |
|---|---|
| `models/vision_model_q4f16.onnx` | Encode thumbnail → 1024-dim vector |
| `models/text_model_q4f16.onnx` | Encode query or tag string → 1024-dim vector |
| `models/text_model_q4f16.onnx_data` | External weight shard (must be co-located with `.onnx`) |
| `models/tokenizer.json` | BPE tokenizer |

### Hybrid Indexing

Each post is stored with **two separate named vectors** in Qdrant:

| Vector name | Source | Represents |
|---|---|---|
| `image` | Vision encoder on preprocessed thumbnail | Visual content |
| `tags` | Text encoder on normalized tag string | Declared subject matter |

At search time, the user's query is encoded once by the text encoder. The result is compared against both vector sets. Results are merged by post ID with a weighted sum:

```
final_score = (image_weight × image_cosine) + (tags_weight × tags_cosine)
```

Default weights: `image_weight = 1.0`, `tags_weight = 0.0` (Fully visual). Adjustable per query via CLI flags.

**Why store both instead of pre-blending:** Weights are adjusted at query time without re-indexing. For content where the visual encoder performs poorly (which is expected for NSFW booru images), a user can lean heavily on tags (`--weight 0.5`). For visually distinctive content, full image weight works well. Neither extreme is the right default for all queries.

**Tag preprocessing:** Booru tags use underscores as word separators. Before encoding, underscores are replaced with spaces: `"short_hair blue_eyes"` → `"short hair blue eyes"`. SigLIP2's tokenizer was trained on natural language; space-separated words produce better embeddings than underscored compound tokens.

---

## Multi-Site Database Design

The Qdrant collection holds posts from multiple booru sites in one collection (`roobu`) without ever requiring a schema migration. New sites are added by assigning a new namespace constant — no collection recreation, no data movement.

### Point ID Namespace

Qdrant point IDs are `u64`. Each site is assigned a permanent numeric namespace. Point IDs are encoded as:

```
point_id = site_namespace × 1_000_000_000_000 + post_id
```

This gives each site a space of one trillion post IDs. No booru site will ever exhaust this. The encoding is deterministic and reversible:

```
site_namespace = point_id / 1_000_000_000_000
post_id        = point_id % 1_000_000_000_000
```

**Assigned namespaces — permanent, never reassign a number:**

| Namespace | Site |
|---|---|
| 1 | `rule34.xxx` |
| 2 | (reserved — e621) |
| 3 | (reserved — Gelbooru) |
| 4 | (reserved — Danbooru) |

Adding a new site means choosing the next unused number. Existing data is never touched.

### Site Filtering

Every point carries an indexed `site` string payload field (e.g. `"rule34"`). Search can be scoped to one site via a Qdrant payload filter on that field. This is declared as a CLI flag (`--site rule34`) and requires no structural change to the collection.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                            roobu                                │
│                                                                 │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐   │
│  │ Rule34   │───▶│  api.rs  │───▶│ ingest.rs│───▶│ embed.rs │   │
│  │ JSON API │    │ reqwest  │    │  loop    │    │          │   │
│  └──────────┘    └──────────┘    └──────────┘    │  vision  │   │
│                                       │          │  (batch) │   │
│                                       ▼          │          │   │
│                                  ┌──────────┐    │  text    │   │
│                                  │ store.rs │    │          │   │
│                                  │  Qdrant  │◀──▶│tokenizer │   │
│                                  └──────────┘    └──────────┘   │
│                                                                 │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐                   │
│  │   User   │───▶│ main.rs  │───▶│ store.rs │                   │
│  │  (query) │    │  (clap)  │    │  search  │                   │
│  └──────────┘    └──────────┘    └──────────┘                   │
│                                                                 │
│  checkpoint   (plain text, one integer per site, auto-created)  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
           ┌───────────────────────────────────────┐
           │               Qdrant                  │
           │  collection: "roobu"                  │
           │  point_id = namespace × 1T + post_id  │
           │  named vectors:                       │
           │    "image" — 1024-dim cosine, on disk │
           │    "tags"  — 1024-dim cosine, on disk │
           │  payload:                             │
           │    post_id (int, indexed)             │
           │    site    (str, indexed)             │
           │    post_url (str)                     │
           │    rating  (str)                      │
           └───────────────────────────────────────┘
```

### Ingest Data Flow

1. Fetch page 0 from the Rule34 JSON API (100 posts, most recent first).
2. Filter to posts with IDs greater than `last_id` from the checkpoint file.
3. Pre-flight filter: skip posts where `has_preview()` is false, or where the aspect ratio derived from `width` and `height` exceeds 2.0.
4. If no posts survive, sleep `poll_interval` seconds and repeat from step 1.
5. For each batch of up to `batch_size` posts:
   a. Download all thumbnails concurrently, bounded by `download_concurrency` semaphore.
   b. Post-download filter: skip if bytes < 500, image decode fails, either dimension < 32px, or aspect ratio (from actual decoded dimensions) > 2.0.
   c. On a blocking thread pool: preprocess all valid images to 256×256 RGB; stack into one `[N, 3, 256, 256]` tensor; run one vision encoder call for the whole batch; run text encoder once per post for tags.
   d. Upsert all `PostEmbedding` records to Qdrant. Point IDs use the namespace encoding.
   e. Atomically write the new `last_id` to the checkpoint file.
6. Sleep `poll_interval` seconds.
7. Repeat from step 1. Always fetch page 0.

**Why always page 0:** The goal is to catch new posts as they appear. Page 0 is always the most recent. Backward crawling through historical pages is not supported in V1.

**Why checkpoint in a local file:** Recovering `last_id` from Qdrant requires a scroll-with-order-by query that depends on the collection existing, the payload index being ready, and the qdrant-client API being used correctly. A local file with one integer has none of those dependencies. It is written atomically (write to `{path}.tmp`, rename). Since Qdrant upserts are idempotent by point ID, a crash between upsert and checkpoint write at worst causes a small number of posts to be re-embedded on the next restart — harmless.

### Search Data Flow

1. Load both ONNX sessions and the Qdrant client.
2. Encode the query string with the text model → 1024-dim unit vector.
3. Run two concurrent Qdrant searches via `tokio::try_join!`: one against `image` vectors, one against `tags` vectors. Each returns `limit × 3` candidates. Both searches optionally include a site payload filter.
4. Merge candidates by point ID using weighted score summation.
5. Sort by merged score descending, take top `limit`, print to stdout.

---

## Image Preprocessing

All images are preprocessed to exactly 256×256 before inference. The FixRes model was trained at this fixed resolution and produces degraded embeddings for any other size.

### Validation Checks (in order)

These checks are applied in two phases. Pre-flight checks use data already available in the API response and avoid unnecessary downloads. Post-download checks run after fetching bytes.

**Pre-flight (from API payload):**

| Check | Action on failure |
|---|---|
| `preview_url` is non-empty | Skip post, no log needed |
| Aspect ratio from `width` / `height` (full image dimensions from API) ≤ 2.0 | Skip post, log at debug with post ID and ratio |

If `width` or `height` are missing or zero in the API response, defer the aspect ratio check to post-download 

**Post-download:**

| Check | Action on failure |
|---|---|
| `bytes.len() >= 500` | Skip post, log warning. Catches placeholder images (1×1 GIF, tiny "image not found" PNG). |
| Image decodes without error | Skip post, log warning with error message. |
| Both decoded dimensions ≥ 32px | Skip post, log warning. |
| Aspect ratio from decoded dimensions ≤ 2.0 | Skip post, log debug with post ID and ratio. (Only applies if pre-flight check was deferred.) |

### Resize and Crop

1. Convert to RGB (`to_rgb8()`).
2. Resize so the shorter dimension equals 256, preserving aspect ratio. Use Lanczos3 filter.
3. Center-crop to 256×256: take the center region, discarding equal margins from both ends of the longer axis.

**Why center crop and not letterbox:** The FixRes SigLIP2 models were trained with center crop preprocessing, as specified in their processor configuration. Letterboxing (padding with a fill color) introduces pixels the model was never trained on. These padding pixels contribute noise to edge patch tokens and produce systematically worse embeddings.

**Why not stretch to 256×256 directly:** Distorting the aspect ratio produces distorted feature activations. Do not do this.

### Normalization

Convert the 256×256 RGB image to a float32 tensor with shape `[1, 3, 256, 256]` (NCHW format, channel-first):

```
normalized_pixel = pixel_u8 / 255.0 * 2.0 - 1.0
```

This maps `[0, 255]` → `[-1.0, 1.0]`. Layout: all red values (channel 0), then green (channel 1), then blue (channel 2), each in row-major order.

### Batch Inference

Because all preprocessed images are 256×256, multiple images can be stacked along axis 0 into a single `[N, 3, 256, 256]` tensor. The ONNX session processes the entire batch in one call. This is significantly more efficient than N individual calls, especially on GPU where per-call kernel overhead dominates for small inputs.

The `embed_images(&[DynamicImage]) -> Result<Vec<[f32; 1024]>>` method accepts a slice of already-preprocessed images and returns one embedding per image. The single-image `embed_image` is a convenience wrapper over this.

### Tags Handling

If a post's tag string is empty or whitespace-only, pass the fallback string `"unknown"` to the text encoder rather than an empty string. A zero-length input produces an undefined or near-zero vector that degrades cosine similarity scoring. Using `"unknown"` produces a consistent, well-defined vector for all tag-empty posts.

---

## File Structure

```
roobu/
├── Cargo.toml
├── checkpoint                         ← last indexed post_id for Rule34 (plain integer)
├── models/
│   ├── vision_model_q4f16.onnx        ← from siglip2-large-patch16-256-ONNX
│   ├── text_model_q4f16.onnx          ← from siglip2-large-patch16-256-ONNX
│   ├── text_model_q4f16.onnx_data     ← must be in same directory as .onnx
│   └── tokenizer.json                 ← from siglip2-large-patch16-256-ONNX
└── src/
    ├── main.rs       ← CLI entry point, subcommand dispatch
    ├── embed.rs      ← ONNX sessions, batch image encoding, text encoding
    ├── api.rs        ← Rule34 HTTP client, Post struct, backoff, validation helpers
    ├── ingest.rs     ← poll loop, download, filter, embed, upsert, checkpoint
    ├── store.rs      ← Qdrant collection, namespace encoding, upsert, search
    └── error.rs      ← RoobuError enum
```

---

## Cargo.toml

```toml
[package]
name         = "roobu"
version      = "0.1.0"
edition      = "2024"
rust-version = "1.88.0"
description  = "Semantic image search for booru sites."
license      = "MIT"

[[bin]]
name = "roobu"
path = "src/main.rs"

[dependencies]
# ── Async Runtime ─────────────────────────────────────────
tokio        = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync", "fs"] }
futures      = "0.3"
# ── ONNX Inference ────────────────────────────────────────
ort          = { version = "2.0.0-rc.12", features = ["cuda", "directml", "coreml", "half", "api-22"] }
ndarray      = "0.17"
tokenizers   = { version = "0.22", default-features = false, features = ["onig"] }
# ── Vector Database Client ────────────────────────────────
qdrant-client = "1"
# ── Image Processing ──────────────────────────────────────
image        = { version = "0.25", default-features = false, features = ["jpeg", "png", "webp", "gif"] }
# ── HTTP Client ───────────────────────────────────────────
reqwest      = { version = "0.13", default-features = false, features = ["rustls", "json", "gzip"] }
# ── Command-Line Interface ─────────────────────────────────
clap         = { version = "4", features = ["derive", "env"] }
# ── Error Handling ─────────────────────────────────────────
anyhow       = "1"
thiserror    = "1"
# ── Serialization ──────────────────────────────────────────
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
# ── Logging and Tracing ────────────────────────────────────
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

**Dependency notes:**

- `ort` feature `api-22` is required for `SessionBuilder::with_auto_device`, which auto-selects CUDA → DirectML → CoreML → CPU at runtime.
- `ndarray` must be `0.17`, matching what `ort` 2.0.0-rc.12 was compiled against. A version mismatch produces type errors at the `inputs!` macro boundary.
- `tokenizers` uses `onig` (Oniguruma) for BPE. Requires the system `onig` library or a vendored build.
- `reqwest` uses `rustls` — no OpenSSL dependency on the VPS.
- `image` enables only the four formats encountered on booru sites. `gif` is included because some previews are animated GIFs; only the first frame is decoded.
- No config file library included. All configuration is via CLI arguments and environment variables.

---

## Module Responsibilities

### `error.rs`

Defines `RoobuError`, a `thiserror` enum:

- `Onnx` — wraps `ort::Error`
- `Tokenizer(String)` — `tokenizers::Error` is not `Send + Sync`, stringified on conversion
- `Qdrant` — wraps `qdrant_client::QdrantError`
- `Http` — wraps `reqwest::Error`
- `Image` — wraps `image::ImageError`
- `Io` — wraps `std::io::Error`
- `Api(String)` — malformed body, HTML error page, JSON parse failure
- `DimensionMismatch { expected, actual }` — unexpected embedding output shape
- `EmptyBatch` — all images in a batch failed validation

`anyhow::Result<T>` is used at CLI handlers. `RoobuError` is used internally.

### `api.rs`

All communication with the Rule34 JSON API.

**Base URL:** `https://api.rule34.xxx/index.php`

**Endpoint used:** `GET /?page=dapi&s=post&q=index&json=1&limit=100&pid=0`

The response is a JSON array. Each element has at minimum: `id` (integer — deserialize directly as `u64`, no parsing step needed), `tags`, `preview_url`, `width`, `height` (full image dimensions), `rating` (full word like "explicit", "safe", not a single letter).

**HTML body guard:** Rule34 occasionally returns HTTP 200 with an HTML error body. Check whether the body is empty or starts with `<` before attempting JSON parse. Return empty `Vec<Post>` in those cases rather than a parse error.

**Backoff:** Retry on 5xx and 429 up to 6 times. Initial delay 5 seconds, doubling each attempt, capped at 300 seconds. Hard-fail after 6 attempts.

**`Post` methods:**
- `post_url() -> String` — canonical post page URL
- `tags_normalized() -> String` — underscores replaced with spaces; returns `"unknown"` if tags are empty
- `has_preview() -> bool` — preview URL is non-empty
- `aspect_ratio() -> Option<f32>` — `max(w, h) / min(w, h)` using `width` and `height` directly; returns `None` only if both are 0
- `is_aspect_ratio_ok(ratio: f32) -> bool` — ratio ≤ 2.0 (static helper, usable for both pre-flight and post-download checks)

### `embed.rs`

Loads both ONNX sessions and the tokenizer. All methods are synchronous and must be called inside `tokio::task::spawn_blocking`.

**Session loading:**
```
Session::builder()?
    .with_intra_threads(N)?       // tune for VPS core count
    .with_inter_threads(1)?
    .with_auto_device(AutoDevicePolicy::MaxPerformance)?
    .commit_from_file(path)?
```

On the VPS (6 cores), `intra_threads = 4` to `6` and `inter_threads = 1` is the recommended starting point for transformer models. Benchmark both.

**Startup logging:** Immediately after loading, log all input and output tensor names for both sessions at `INFO` level. This is the primary diagnostic for unexpected model behavior.

**Public interface:**
- `embed_images(images: &[DynamicImage]) -> Result<Vec<[f32; 1024]>>` — batch vision path. All images must already be 256×256 RGB. Stacks into `[N, 3, 256, 256]` tensor, one ONNX call, extracts all rows from `pooler_output`.
- `embed_image(img: &DynamicImage) -> Result<[f32; 1024]>` — single-image wrapper over `embed_images`.
- `embed_text(text: &str) -> Result<[f32; 1024]>` — tokenize, pad/truncate to 64 tokens, run inference, extract from `pooler_output`.

All outputs are L2-normalized defensively after extraction, regardless of the model's stated normalization behavior.

The `Embedder` struct is `Send + Sync` and is shared across the ingest loop via `Arc`.

### `store.rs`

Wraps the Qdrant client. Manages the collection and exposes upsert and search.

**Namespace constants:**
```rust
pub const SITE_RULE34: u64 = 1;
// future: pub const SITE_E621: u64 = 2;

pub fn encode_point_id(site_ns: u64, post_id: u64) -> u64 {
    site_ns * 1_000_000_000_000 + post_id
}

pub fn decode_point_id(point_id: u64) -> (u64, u64) {
    (point_id / 1_000_000_000_000, point_id % 1_000_000_000_000)
}
```

**Collection creation:** Named vectors `image` and `tags`, both 1024-dim cosine, both stored on disk. Immediately after collection creation, create payload indexes on `post_id` (integer) and `site` (keyword).

**`upsert(posts: Vec<PostEmbedding>)`:** Point ID is `encode_point_id(site_ns, post_id)`. Payload: `post_id` (raw, without namespace), `site`, `post_url`, `rating`.

**`search(query_vec, image_weight, tags_weight, limit, site_filter: Option<&str>)`:** Two concurrent searches via `tokio::try_join!`. The optional `site_filter` translates to a Qdrant `Filter` with a `FieldCondition` match on `site`, applied to both searches. Client-side weighted score merge, sort descending, truncate to `limit`.

**Qdrant client API (`qdrant-client = "1"`):** `Qdrant::from_url()`, `CreateCollectionBuilder`, `VectorsConfigBuilder`, `VectorParamsBuilder`, `UpsertPointsBuilder`, `SearchPointsBuilder`. Payload accessed via `.as_integer()` and `.as_str()`.

**What is not stored in payload:** The tags string (too large at scale; retrievable from post URL). The preview URL (not needed in search output).

### `ingest.rs`

**Checkpoint functions:**
- `load_checkpoint(path: &str) -> u64` — reads the file, parses to u64, returns 0 on any failure
- `save_checkpoint(path: &str, id: u64)` — writes to `{path}.tmp` then renames atomically

**`run(...)` function:** The main poll loop. Parameters passed directly from CLI args: `qdrant_url`, `models_dir`, `checkpoint`, `poll_interval_secs`, `batch_size`, `download_concurrency`.

**`download_batch`:** Concurrent downloads using `futures::stream::buffer_unordered` and `tokio::sync::Semaphore`. Each download is independent; a failure logs a warning and skips that post without aborting the batch.

**Embedding call site:** Inside `tokio::task::spawn_blocking`. The closure receives an `Arc<Embedder>` clone. All preprocessing (resize, center crop, normalization) happens inside the blocking closure alongside inference. Returns `Vec<PostEmbedding>`.

**Checkpoint saved after each batch**, not after the full page.

### `main.rs`

Two subcommands defined with `clap` derive macros.

**`ingest`**
- `--qdrant-url` (env `QDRANT_URL`, default `http://localhost:6333`)
- `--models-dir` (default `models`)
- `--checkpoint` (default `checkpoint`)
- `--poll-interval` (default `60`, seconds between polls when no new posts are found)
- `--batch-size` (default `16`)
- `--download-concurrency` (default `8`)

**`search`**
- `query` (positional)
- `--limit` / `-l` (default `10`)
- `--qdrant-url` (env `QDRANT_URL`, default `http://localhost:6333`)
- `--models-dir` (default `models`)
- `--weight` (default `1.0`)
- `--site` (optional string, default: all sites)

Log level is controlled by `RUST_LOG`, defaulting to `roobu=info`.

---

## Qdrant Collection Schema

**Collection name:** `roobu`

**Vectors:**

| Name | Dimensions | Distance | On disk |
|---|---|---|---|
| `image` | 1024 | Cosine | Yes |
| `tags` | 1024 | Cosine | Yes |

Both vectors stored on disk to keep RAM usage low on the constrained VPS. Qdrant memory-maps them; access is still fast for typical collection sizes.

**Payload fields:**

| Field | Type | Indexed | Notes |
|---|---|---|---|
| `post_id` | integer | Yes | Raw site post ID, without namespace |
| `site` | string | Yes | `"rule34"`, `"e621"`, etc. |
| `post_url` | string | No | Canonical post page URL, returned in results |
| `rating` | string | No | Content rating from API (`s`, `q`, `e`) |

**Point ID:** `site_namespace × 1_000_000_000_000 + post_id`. Namespaces are permanent; Rule34 = 1.

**Disk usage estimate:** ~6KB per point (1024 × 4 bytes × 2 vectors + ~400 bytes payload). 1 million posts ≈ 6GB. 10 million posts ≈ 60GB.

---

## CLI Usage

```bash
# Index Rule34 with Qdrant at default location
roobu ingest

# Custom Qdrant URL and larger batches
roobu ingest --qdrant-url http://my-vps:6333 --batch-size 32

# Search across all sites
roobu search "a dragon breathing fire"

# Search only Rule34 results
roobu search "blonde elf archer" --site rule34

# More results, tag-heavy weights (useful when image encoder underperforms)
roobu search "red dress" --limit 20 --weight 0.7

# Pure visual similarity
roobu search "outdoor forest scene" --weight 1.0

# Override Qdrant URL via environment variable
QDRANT_URL=http://my-vps:6333 roobu search "cat ears"
```

**Search output format (one line per result):**
```
#7823412    0.8934  https://rule34.xxx/index.php?page=post&s=view&id=7823412
#6104857    0.8712  https://rule34.xxx/index.php?page=post&s=view&id=6104857
```

---

## Milestones

### M0 — Model Loading and Tensor Shape Verification

**Goal:** Both ONNX sessions load. Input/output tensor names and shapes logged and correct.

**Steps:**
1. Place all four model files in `models/`.
2. Implement `Embedder::new` with session loading and startup `INFO` logging of all inputs and outputs.
3. Implement a minimal `main.rs` that constructs the embedder and exits.
4. Run and inspect the logs.
5. Confirm `pixel_values` is the only vision input, `input_ids` is the only text input, and both sessions output `pooler_output` with shape `[batch, 1024]`.

**Pass criteria:** Sessions load on target hardware without error. Correct tensor names in logs.

---

### M1 — Embedding Space Sanity Check

**Goal:** Both encoders produce valid L2-normalized vectors, and the embedding space is aligned.

**Steps:**
1. Implement full `embed_images` and `embed_text` including all preprocessing.
2. Download two thumbnails manually: one that clearly matches a query (e.g. a dragon post, query `"dragon"`), one that clearly does not (e.g. a portrait, query `"dragon"`).
3. Apply the full preprocessing pipeline to both images.
4. Compute `dot(query_vec, image_vec)` for both pairs (dot product of unit vectors equals cosine similarity).
5. The matching pair must score noticeably higher. A gap of ≥ 0.05 is sufficient. A gap near zero means the embedding space is not aligned and the model combination must be reconsidered.
6. Confirm all output vectors have L2 norm between 0.999 and 1.001.
7. Confirm no NaN or Inf values in any output.

**Pass criteria:** Aligned embedding space confirmed. This is the go/no-go gate for the model choice.

---

### M2 — Rule34 API Client

**Goal:** Posts fetched and parsed correctly. Filters work.

**Steps:**
1. Implement `Rule34Client`, `fetch_page_backoff`, and the `Post` struct with all helper methods.
2. Fetch page 0 and print 5 post IDs, preview URLs, tag strings (normalized), and aspect ratios from API payload.
3. Verify `tags_normalized()` replaces underscores and returns `"unknown"` for empty tags.
4. Verify `preview_aspect_ratio()` and `is_aspect_ratio_ok()` correctly identify a wide-format post (if one exists in the page) and reject it.
5. Test the HTML-body guard: point the URL at an endpoint returning HTML; confirm it returns an empty `Vec` rather than an error.

**Pass criteria:** 100 posts fetched. IDs parse correctly. Tag normalization, aspect ratio filter, and HTML guard all work.

---

### M3 — Qdrant Collection and Round-Trip

**Goal:** Collection created with named vectors and namespace-encoded point IDs. Upsert and search confirmed correct.

**Steps:**
1. Implement `Store::new`, `ensure_collection`, `encode_point_id`, `decode_point_id`.
2. Manually construct a `PostEmbedding` for a Rule34 post (namespace 1) using real vectors from M1.
3. Upsert it. Confirm the stored point ID is `1_000_000_000_000 + post_id` by inspecting Qdrant directly.
4. Search with the query vector used in M1. Confirm the point is returned with a plausible score.
5. Restart. Confirm `ensure_collection` does not recreate the collection or reset the payload index.
6. Search with `--site rule34` filter; confirm the point is returned. Search with a different site filter; confirm no results.

**Pass criteria:** Namespace encoding correct. Round-trip works. Site filter works. Collection survives restart.

---

### M4 — Full Ingest Pipeline (One Batch)

**Goal:** One batch flows end-to-end through all stages.

**Steps:**
1. Implement `ingest::run` fully, including both validation filter stages.
2. Run with `--batch-size 8`.
3. Observe log sequence: fetch → pre-flight filter → download → post-download filter → batch embed → upsert → checkpoint write.
4. Confirm posts rejected by the aspect ratio filter appear in debug logs.
5. Confirm Qdrant point count increased by the number of valid posts.
6. Read the checkpoint file and confirm the correct `last_id`.
7. Kill. Restart. Confirm no re-indexing (Qdrant count unchanged, process polls normally).

**Pass criteria:** End-to-end correct. Both filter stages work. Checkpoint persists and recovers.

---

### M5 — Unattended Operation

**Goal:** Ingest runs continuously without crashing, memory leak, or duplicate entries.

**Steps:**
1. Run ingest on the VPS for 30 minutes with default settings.
2. Monitor Qdrant point count via `GET /collections/roobu` on the Qdrant HTTP API.
3. Monitor RSS memory of the roobu process over time. Should be roughly stable after the first few batches.
4. Kill and restart. Confirm smooth resume from checkpoint.

**Pass criteria:** No crashes. No memory growth trend. Correct resume. Point count grows monotonically.

---

### M6 — Search Quality

**Goal:** Natural language queries return semantically relevant results. Weights and filters behave as expected.

**Steps:**
1. Ensure at least 1,000 posts are indexed.
2. Run several queries: a common subject, a color-subject combination, a visual style, a scene description.
3. Open returned URLs in a browser and visually inspect relevance.
4. Compare `--weight 1.0` vs `--weight 0.0` on the same query. Tag-only should behave like keyword search. Image-only should be purely visual.
5. Run the same query with `--site rule34`; confirm only Rule34 results appear.
6. Confirm results are sorted by score descending.

**Pass criteria:** Top results are plausibly relevant. Weight extremes produce observably different result sets. Site filter is correct.

---

## Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Ingest throughput (CPU, VPS) | ≥ 60 posts/min | Includes download time, batch size 16 |
| Ingest throughput (GPU, dev) | ≥ 300 posts/min | CUDA, batch size 16+ |
| Text embedding latency | < 100ms | Per query, CPU |
| Search latency (1M posts) | < 500ms | Qdrant on localhost, two concurrent searches |
| RAM during ingest (CPU) | < 1.5GB RSS | Vectors on disk in Qdrant |
| Disk per indexed post | ~6KB | Two 1024-dim float32 vectors + payload |

---

## Known Limitations (V1)

- **Model domain mismatch:** SigLIP2 was trained on general web images, not NSFW booru content. The visual encoder will underperform on that content. Tag-based hybrid search compensates partially. This is a fundamental limitation that cannot be resolved without model fine-tuning or replacement.
- **Page 0 only:** Only the 100 most recent posts are checked per poll cycle. No historical backfill. Initial corpus is built by running ingest from a clean checkpoint and letting it accumulate over time.
- **Single site implemented:** Rule34 only in V1. The schema accommodates more sites without migration.
- **No deletion:** Removed posts remain in Qdrant until the collection is manually dropped and rebuilt.
- **No re-indexing:** Changed thumbnails leave stale vectors until manually reindexed.
- **Tag truncation:** Posts with many tags lose the most specific tags to the 64-token context window cutoff.
- **No Qdrant authentication:** Assumes a trusted network. Configure Qdrant API key authentication for any internet-exposed deployment.

---

## Extensibility

### Adding a New Booru Site

1. Add a new permanent namespace constant in `store.rs`: `pub const SITE_E621: u64 = 2;`
2. Create a new API client returning `Vec<Post>` (or a shared post type with site-specific deserialization).
3. Add a site selector flag to the `ingest` subcommand and a separate checkpoint file per site (e.g. `checkpoint_rule34`, `checkpoint_e621`).
4. No changes to the collection schema, vector configuration, embedder, or search logic.

### Adding Content Rating Filter to Search

`rating` is already in the payload. Add a `--rating s` flag to the `search` subcommand that appends a `FieldCondition` on `rating` to the Qdrant filter passed to both vector searches.

### Upgrading to Qdrant Native Hybrid Fusion

Qdrant's Query API supports Reciprocal Rank Fusion natively via prefetch + fusion, which would replace the current client-side score merging with a single API call. When this stabilizes in `qdrant-client = "1"`, `store::search` can be simplified accordingly.

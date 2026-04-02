//! Application-wide constants and default configuration values.
//!
//! This module centralizes all tunable parameters so they can be easily
//! discovered and adjusted. Values here are used as CLI argument defaults
//! and as fallbacks when environment variables are not set.

// ── Connection & Paths ──────────────────────────────────────────────────────

/// Default Qdrant gRPC endpoint URL.
pub const DEFAULT_QDRANT_URL: &str = "http://localhost:6334";
/// Default directory containing ONNX model files and tokenizer.
pub const DEFAULT_MODELS_DIR: &str = "models";
/// Default path to the ingest checkpoint file.
pub const DEFAULT_CHECKPOINT_PATH: &str = "checkpoint.json";

// ── Ingest Tuning ───────────────────────────────────────────────────────────

/// Default polling interval between site fetch cycles (seconds).
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
/// Default maximum posts per embed/upsert batch.
pub const DEFAULT_BATCH_SIZE: usize = 8;
/// Default maximum concurrent image downloads.
pub const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 8;
/// Default per-site timeout for fetching new posts (seconds).
pub const DEFAULT_INGEST_FETCH_TIMEOUT_SECS: u64 = 20;

// ── Search ──────────────────────────────────────────────────────────────────

/// Default maximum number of search results to return.
pub const DEFAULT_SEARCH_LIMIT: u64 = 10;
/// Default weight for the image component in hybrid text+image queries.
pub const DEFAULT_IMAGE_WEIGHT: f32 = 1.0;

// ── Clustering ──────────────────────────────────────────────────────────────

/// Default page size for fetching points from Qdrant during clustering.
pub const DEFAULT_CLUSTER_PAGE_SIZE: u32 = 256;
/// Default maximum number of points to fetch before clustering.
pub const DEFAULT_CLUSTER_MAX_POINTS: usize = 50_000;
/// Default minimum number of samples required to form a cluster.
pub const DEFAULT_CLUSTER_MIN_CLUSTER_SIZE: usize = 10;
/// Default number of top clusters to display.
pub const DEFAULT_CLUSTER_TOP_CLUSTERS: usize = 10;
/// Default projection dimensionality before clustering (lower is faster).
pub const DEFAULT_CLUSTER_PROJECTION_DIMS: usize = 256;
/// Default cohesion threshold below which clusters are considered low-quality.
pub const DEFAULT_CLUSTER_LOW_COHESION_THRESHOLD: f64 = 0.75;
/// Default number of non-zero entries in the random projection matrix.
pub const DEFAULT_CLUSTER_PROJECTION_NNZ: usize = 2;
/// Default seed for the random projection matrix.
pub const DEFAULT_CLUSTER_PROJECTION_SEED: u64 = 1_215_765_097;
/// Default number of neighbors in the approximate KNN graph.
pub const DEFAULT_CLUSTER_GRAPH_NEIGHBORS: usize = 32;
/// Default number of pivots for distance estimation.
pub const DEFAULT_CLUSTER_GRAPH_PIVOTS: usize = 64;
/// Default number of top pivots to use for distance estimation.
pub const DEFAULT_CLUSTER_GRAPH_TOP_PIVOTS: usize = 3;
/// Default maximum candidate pool size for graph construction.
pub const DEFAULT_CLUSTER_GRAPH_MAX_CANDIDATES: usize = 512;
/// Default epsilon for mutual reachability distance.
pub const DEFAULT_CLUSTER_GRAPH_EPSILON: f32 = 0.05;

// ── Stats ───────────────────────────────────────────────────────────────────

/// Default page size for scrolling through Qdrant points during stats.
pub const DEFAULT_STATS_PAGE_SIZE: u32 = 1024;
/// Default width of the ASCII bar chart in stats output.
pub const DEFAULT_STATS_BAR_WIDTH: usize = 48;

// ── Tracing ─────────────────────────────────────────────────────────────────

/// Default tracing filter string (RUST_LOG format).
pub const DEFAULT_TRACING_FILTER: &str = "roobu=info";

// ── Qdrant Schema ───────────────────────────────────────────────────────────

/// Name of the Qdrant collection used for storing embeddings.
pub const QDRANT_COLLECTION: &str = "roobu";
/// Multiplier used to encode (site_namespace, post_id) into a single u64 point ID.
///
/// The site namespace occupies the high-order digits, allowing up to
/// 999,999,999,999 post IDs per site.
pub const POINT_ID_SITE_MULTIPLIER: u64 = 1_000_000_000_000;
/// Multiplier applied to the search limit to fetch extra candidates before
/// post-filtering by site.
pub const SEARCH_FETCH_LIMIT_MULTIPLIER: u64 = 3;

// ── SigLIP Model Parameters ─────────────────────────────────────────────────

/// Expected input image size for the SigLIP vision model (square, pixels).
pub const SIGLIP_IMAGE_SIZE: u32 = 256;
/// Expected text sequence length for the SigLIP text model (tokens).
pub const SIGLIP_TEXT_SEQ_LEN: usize = 64;

// ── Image Validation ────────────────────────────────────────────────────────

/// Minimum file size for a downloaded image to be considered valid (bytes).
pub const MIN_DOWNLOADED_IMAGE_BYTES: usize = 500;
/// Minimum edge dimension (pixels) for an image to be considered valid.
pub const MIN_IMAGE_EDGE_PX: u32 = 32;
/// Maximum allowed aspect ratio (longer/shorter edge) before an image is skipped.
pub const MAX_IMAGE_ASPECT_RATIO: f32 = 2.0;

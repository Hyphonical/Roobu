pub const DEFAULT_QDRANT_URL: &str = "http://localhost:6334";
pub const DEFAULT_MODELS_DIR: &str = "models";
pub const DEFAULT_CHECKPOINT_PATH: &str = "checkpoint.json";
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
pub const DEFAULT_BATCH_SIZE: usize = 16;
pub const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 8;
pub const DEFAULT_SEARCH_LIMIT: u64 = 10;
pub const DEFAULT_IMAGE_WEIGHT: f32 = 1.0;
pub const DEFAULT_CLUSTER_PAGE_SIZE: u32 = 256;
pub const DEFAULT_CLUSTER_MAX_POINTS: usize = 50_000;
pub const DEFAULT_CLUSTER_MIN_CLUSTER_SIZE: usize = 10;
pub const DEFAULT_CLUSTER_PREVIEW_LIMIT: usize = 10;
pub const DEFAULT_CLUSTER_EPSILON: f64 = 0.05;
pub const DEFAULT_CLUSTER_LOW_COHESION_THRESHOLD: f64 = 0.75;
pub const DEFAULT_TRACING_FILTER: &str = "roobu=info";

pub const QDRANT_COLLECTION: &str = "roobu";
pub const POINT_ID_SITE_MULTIPLIER: u64 = 1_000_000_000_000;
pub const SEARCH_FETCH_LIMIT_MULTIPLIER: u64 = 3;

pub const SIGLIP_IMAGE_SIZE: u32 = 256;
pub const SIGLIP_TEXT_SEQ_LEN: usize = 64;

pub const MIN_DOWNLOADED_IMAGE_BYTES: usize = 500;
pub const MIN_IMAGE_EDGE_PX: u32 = 32;
pub const MAX_IMAGE_ASPECT_RATIO: f32 = 2.0;

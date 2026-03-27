use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{config, embed};

#[derive(Parser)]
#[command(
	name = "roobu",
	version,
	about = "Semantic image search for booru sites."
)]
pub struct Cli {
	#[command(subcommand)]
	pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
	Ingest {
		#[arg(
			long,
			env = "QDRANT_URL",
			default_value = config::DEFAULT_QDRANT_URL,
			help = "Qdrant gRPC endpoint URL"
		)]
		qdrant_url: String,

		#[arg(long, default_value = config::DEFAULT_MODELS_DIR, help = "Directory containing ONNX model files")]
		models_dir: PathBuf,

		#[arg(long, default_value = config::DEFAULT_CHECKPOINT_PATH, help = "Checkpoint file for ingest resume state")]
		checkpoint: PathBuf,

		#[arg(long, default_value_t = config::DEFAULT_POLL_INTERVAL_SECS, help = "Polling interval between site fetch cycles (seconds)")]
		poll_interval: u64,

		#[arg(long, default_value_t = config::DEFAULT_BATCH_SIZE, help = "Maximum posts to process per embed/upsert batch")]
		batch_size: usize,

		#[arg(long, default_value_t = config::DEFAULT_DOWNLOAD_CONCURRENCY, help = "Maximum number of concurrent image downloads")]
		download_concurrency: usize,

		#[arg(long, env = "RULE34_API_KEY", help = "Rule34 API key")]
		api_key: String,

		#[arg(long, env = "RULE34_USER_ID", help = "Rule34 user ID")]
		user_id: String,

		#[arg(
			long,
			env = "ROOBU_ONNX_OPTIMIZATION",
			value_enum,
			default_value_t = embed::OnnxOptimizationIntensity::Safe,
			help = "ONNX graph optimization level: safe, balanced, or aggressive"
		)]
		onnx_optimization: embed::OnnxOptimizationIntensity,
	},

	Search {
		#[arg(
			required_unless_present = "image",
			help = "Text query used for semantic search"
		)]
		query: Option<String>,

		#[arg(
			short = 'i',
			long = "image",
			value_name = "PATH",
			required_unless_present = "query",
			help = "Image path used as a visual query"
		)]
		image: Option<PathBuf>,

		#[arg(short, long, default_value_t = config::DEFAULT_SEARCH_LIMIT, help = "Maximum number of results to return")]
		limit: u64,

		#[arg(
			long,
			env = "QDRANT_URL",
			default_value = config::DEFAULT_QDRANT_URL,
			help = "Qdrant gRPC endpoint URL"
		)]
		qdrant_url: String,

		#[arg(long, default_value = config::DEFAULT_MODELS_DIR, help = "Directory containing ONNX model files")]
		models_dir: PathBuf,

		#[arg(long, default_value_t = config::DEFAULT_IMAGE_WEIGHT, help = "Image-vector weight in [0.0, 1.0]; tag weight is computed as 1.0 - weight")]
		weight: f32,

		#[arg(
			long,
			env = "ROOBU_ONNX_OPTIMIZATION",
			value_enum,
			default_value_t = embed::OnnxOptimizationIntensity::Safe,
			help = "ONNX graph optimization level: safe, balanced, or aggressive"
		)]
		onnx_optimization: embed::OnnxOptimizationIntensity,

		#[arg(
			long,
			help = "Restrict search to a specific site payload value (for example: rule34)"
		)]
		site: Option<String>,
	},

	Cluster {
		#[arg(
			long,
			env = "QDRANT_URL",
			default_value = config::DEFAULT_QDRANT_URL,
			help = "Qdrant gRPC endpoint URL"
		)]
		qdrant_url: String,

		#[arg(
			long,
			help = "Restrict clustering to a specific site payload value (for example: rule34)"
		)]
		site: Option<String>,

		#[arg(long, default_value_t = config::DEFAULT_CLUSTER_PAGE_SIZE, help = "Qdrant scroll page size to limit per-request load")]
		page_size: u32,

		#[arg(long, default_value_t = config::DEFAULT_CLUSTER_MAX_POINTS, help = "Maximum number of points to fetch before running HDBSCAN")]
		max_points: usize,

		#[arg(long, default_value_t = config::DEFAULT_CLUSTER_MIN_CLUSTER_SIZE, help = "Minimum number of samples required to form a cluster")]
		min_cluster_size: usize,

		#[arg(
			long,
			help = "Optional min_samples override for core-distance neighborhood size"
		)]
		min_samples: Option<usize>,

		#[arg(
			long,
			help = "Allow a single dominant cluster when data strongly supports it"
		)]
		allow_single_cluster: bool,
	},
}

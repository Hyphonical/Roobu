use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{config, embed, sites::SiteKind};

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
			value_enum,
			help = "Site to ingest from (rule34, e621, safebooru, xbooru, kemono, aibooru, danbooru, civitai, e6ai, gelbooru, konachan, or yandere). If omitted, ingests supported sites sequentially"
		)]
		site: Option<SiteKind>,

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

		#[arg(
			long,
			default_value_t = config::DEFAULT_INGEST_FETCH_TIMEOUT_SECS,
			help = "Per-site timeout for fetching new posts before skipping the site (seconds)"
		)]
		site_fetch_timeout_secs: u64,

		#[arg(
			long,
			visible_alias = "api-key",
			env = "RULE34_API_KEY",
			help = "Rule34 API key (required when --site rule34)"
		)]
		rule34_api_key: Option<String>,

		#[arg(
			long,
			visible_alias = "user-id",
			env = "RULE34_USER_ID",
			help = "Rule34 user ID (required when --site rule34)"
		)]
		rule34_user_id: Option<String>,

		#[arg(
			long,
			env = "E621_LOGIN",
			help = "e621 login (optional, must be paired with --e621-api-key)"
		)]
		e621_login: Option<String>,

		#[arg(
			long,
			env = "E621_API_KEY",
			help = "e621 API key (optional, must be paired with --e621-login)"
		)]
		e621_api_key: Option<String>,

		#[arg(
			long,
			env = "GELBOORU_API_KEY",
			help = "Gelbooru API key (required when --site gelbooru; optional in all-sites mode)"
		)]
		gelbooru_api_key: Option<String>,

		#[arg(
			long,
			env = "GELBOORU_USER_ID",
			help = "Gelbooru user ID (required when --site gelbooru; optional in all-sites mode)"
		)]
		gelbooru_user_id: Option<String>,

		#[arg(
			long,
			env = "KEMONO_SESSION",
			help = "Kemono session token used as cookie value (optional, can improve feed freshness)"
		)]
		kemono_session: Option<String>,

		#[arg(
			long,
			env = "KEMONO_BASE_URL",
			help = "Kemono base URL override (optional, for domain changes)"
		)]
		kemono_base_url: Option<String>,

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

		#[arg(long, default_value_t = config::DEFAULT_IMAGE_WEIGHT, help = "Image-query weight in [0.0, 1.0] for text+image hybrid queries; text-query weight is 1.0 - weight")]
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

		#[arg(long, default_value_t = config::DEFAULT_CLUSTER_MAX_POINTS, help = "Maximum number of points to fetch before clustering")]
		max_points: usize,

		#[arg(long, default_value_t = config::DEFAULT_CLUSTER_MIN_CLUSTER_SIZE, help = "Minimum number of samples required to form a cluster")]
		min_cluster_size: usize,

		#[arg(
			long,
			default_value_t = config::DEFAULT_CLUSTER_PROJECTION_DIMS,
			help = "Projection dimension used before clustering (lower is faster)"
		)]
		projection_dims: usize,

		#[arg(
			long,
			default_value_t = config::DEFAULT_CLUSTER_TOP_CLUSTERS,
			help = "Maximum number of most cohesive clusters to display"
		)]
		top_clusters: usize,
	},

	Stats {
		#[arg(
			long,
			env = "QDRANT_URL",
			default_value = config::DEFAULT_QDRANT_URL,
			help = "Qdrant gRPC endpoint URL"
		)]
		qdrant_url: String,

		#[arg(long, default_value_t = config::DEFAULT_STATS_PAGE_SIZE, help = "Qdrant scroll page size used while counting points")]
		page_size: u32,

		#[arg(long, default_value_t = config::DEFAULT_STATS_BAR_WIDTH, help = "Maximum width of ASCII bars in terminal output")]
		width: usize,
	},
}

#[cfg(test)]
mod tests {
	use clap::Parser;

	use super::{Cli, Commands};
	use crate::config;
	use crate::sites::SiteKind;

	#[test]
	fn ingest_without_site_uses_all_sites_mode() {
		let cli = Cli::try_parse_from(["roobu", "ingest"]).expect("ingest args should parse");

		match cli.command {
			Commands::Ingest { site, .. } => assert_eq!(site, None),
			_ => panic!("expected ingest command"),
		}
	}

	#[test]
	fn ingest_with_site_uses_requested_site() {
		let cli = Cli::try_parse_from(["roobu", "ingest", "--site", "e621"])
			.expect("ingest args with site should parse");

		match cli.command {
			Commands::Ingest { site, .. } => assert_eq!(site, Some(SiteKind::E621)),
			_ => panic!("expected ingest command"),
		}
	}

	#[test]
	fn ingest_with_new_site_value_parses() {
		let cli = Cli::try_parse_from(["roobu", "ingest", "--site", "e6ai"])
			.expect("ingest args with new site should parse");

		match cli.command {
			Commands::Ingest { site, .. } => assert_eq!(site, Some(SiteKind::E6Ai)),
			_ => panic!("expected ingest command"),
		}
	}

	#[test]
	fn stats_command_parses_with_defaults() {
		let cli = Cli::try_parse_from(["roobu", "stats"]).expect("stats args should parse");

		match cli.command {
			Commands::Stats {
				qdrant_url,
				page_size,
				width,
			} => {
				assert_eq!(qdrant_url, config::DEFAULT_QDRANT_URL);
				assert_eq!(page_size, config::DEFAULT_STATS_PAGE_SIZE);
				assert_eq!(width, config::DEFAULT_STATS_BAR_WIDTH);
			}
			_ => panic!("expected stats command"),
		}
	}

	#[test]
	fn cluster_command_parses_with_defaults() {
		let cli = Cli::try_parse_from(["roobu", "cluster"]).expect("cluster args should parse");

		match cli.command {
			Commands::Cluster {
				max_points,
				min_cluster_size,
				projection_dims,
				top_clusters,
				..
			} => {
				assert_eq!(max_points, config::DEFAULT_CLUSTER_MAX_POINTS);
				assert_eq!(min_cluster_size, config::DEFAULT_CLUSTER_MIN_CLUSTER_SIZE);
				assert_eq!(projection_dims, config::DEFAULT_CLUSTER_PROJECTION_DIMS);
				assert_eq!(top_clusters, config::DEFAULT_CLUSTER_TOP_CLUSTERS);
			}
			_ => panic!("expected cluster command"),
		}
	}
}

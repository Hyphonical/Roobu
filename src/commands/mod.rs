//! Command dispatchers for the CLI.
//!
//! Each subcommand (ingest, search, cluster, stats, serve) has its own module
//! with a `run` function. This module matches the parsed CLI arguments and
//! delegates to the appropriate handler.

mod cluster;
mod ingest;
mod search;
mod serve;
mod stats;

use crate::cli::Commands;

/// Dispatch to the appropriate command handler based on the parsed CLI arguments.
pub async fn run(command: Commands) -> anyhow::Result<()> {
	match command {
		Commands::Ingest {
			site,
			qdrant_url,
			models_dir,
			checkpoint,
			poll_interval,
			batch_size,
			download_concurrency,
			site_fetch_timeout_secs,
			rule34_api_key,
			rule34_user_id,
			e621_login,
			e621_api_key,
			gelbooru_api_key,
			gelbooru_user_id,
			kemono_session,
			kemono_base_url,
			onnx_optimization,
		} => {
			ingest::run(ingest::Args {
				site,
				qdrant_url,
				models_dir,
				checkpoint,
				poll_interval,
				batch_size,
				download_concurrency,
				site_fetch_timeout_secs,
				rule34_api_key,
				rule34_user_id,
				e621_login,
				e621_api_key,
				gelbooru_api_key,
				gelbooru_user_id,
				kemono_session,
				kemono_base_url,
				onnx_optimization,
			})
			.await
		}
		Commands::Search {
			query,
			image,
			limit,
			qdrant_url,
			models_dir,
			weight,
			onnx_optimization,
			site,
		} => {
			search::run(search::Args {
				query,
				image,
				limit,
				qdrant_url,
				models_dir,
				weight,
				onnx_optimization,
				site,
			})
			.await
		}
		Commands::Cluster {
			qdrant_url,
			site,
			max_points,
			min_cluster_size,
			projection_dims,
			top_clusters,
		} => {
			cluster::run(cluster::Args {
				qdrant_url,
				site,
				max_points,
				min_cluster_size,
				projection_dims,
				top_clusters,
			})
			.await
		}
		Commands::Stats {
			qdrant_url,
			page_size,
			width,
		} => {
			stats::run(stats::Args {
				qdrant_url,
				page_size,
				width,
			})
			.await
		}
		Commands::Serve {
			qdrant_url,
			models_dir,
			address,
			onnx_optimization,
		} => {
			serve::run(serve::Args {
				qdrant_url,
				models_dir,
				address,
				onnx_optimization,
			})
			.await
		}
	}
}

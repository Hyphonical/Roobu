mod cluster;
mod ingest;
mod search;

use crate::cli;

pub async fn run(command: cli::Commands) -> anyhow::Result<()> {
	match command {
		cli::Commands::Ingest {
			qdrant_url,
			models_dir,
			checkpoint,
			poll_interval,
			batch_size,
			download_concurrency,
			api_key,
			user_id,
			onnx_optimization,
		} => {
			ingest::run(ingest::Args {
				qdrant_url,
				models_dir,
				checkpoint,
				poll_interval,
				batch_size,
				download_concurrency,
				api_key,
				user_id,
				onnx_optimization,
			})
			.await
		}
		cli::Commands::Search {
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
		cli::Commands::Cluster {
			qdrant_url,
			site,
			page_size,
			max_points,
			min_cluster_size,
			min_samples,
			limit,
			max_cluster_size,
			epsilon,
			allow_single_cluster,
		} => {
			cluster::run(cluster::Args {
				qdrant_url,
				site,
				page_size,
				max_points,
				min_cluster_size,
				min_samples,
				limit,
				max_cluster_size,
				epsilon,
				allow_single_cluster,
			})
			.await
		}
	}
}

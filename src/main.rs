mod checkpoint;
mod cli;
mod config;
mod embed;
mod error;
mod ingest;
mod sites;
mod store;
#[macro_use]
mod ui;

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{bail, ensure};
use clap::Parser;
use hdbscan::{Hdbscan, HdbscanHyperParams};
use image::ImageReader;
use owo_colors::OwoColorize;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| config::DEFAULT_TRACING_FILTER.parse().unwrap()),
		)
		.init();

	let cli = cli::Cli::parse();

	match cli.command {
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
			ui::header("roobu · init");

			ui_step!("{}", "Loading embedder…");
			let embedder = Arc::new(embed::Embedder::new(
				&models_dir,
				embed::ModelLoad::TextAndVision,
				onnx_optimization,
			)?);
			ui_success!("Embedder ready");

			ui_step!("{}", "Connecting to Qdrant…");
			let store = store::Store::new(&qdrant_url).await?;
			ui_success!("Qdrant ready");

			let client = sites::rule34::Rule34Client::new(api_key, user_id)?;

			let config = ingest::IngestConfig {
				poll_interval_secs: poll_interval,
				batch_size,
				download_concurrency,
			};

			ingest::run(client, &store, embedder, &checkpoint, &config).await?;
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
			ui::header("roobu · search");

			ensure!(
				(0.0..=1.0).contains(&weight),
				"--weight must be between 0.0 and 1.0"
			);

			let text_query = query
				.as_deref()
				.map(str::trim)
				.filter(|q| !q.is_empty())
				.map(ToOwned::to_owned);
			let has_text_query = text_query.is_some();
			let has_image_query = image.is_some();

			if !has_text_query && !has_image_query {
				bail!("provide a text query, --image, or both");
			}

			let model_load = match (has_text_query, has_image_query) {
				(true, true) => embed::ModelLoad::TextAndVision,
				(true, false) => embed::ModelLoad::TextOnly,
				(false, true) => embed::ModelLoad::VisionOnly,
				(false, false) => unreachable!("validated above"),
			};

			let embedder = Arc::new(embed::Embedder::new(
				&models_dir,
				model_load,
				onnx_optimization,
			)?);
			let store = store::Store::new(&qdrant_url).await?;

			let image_weight = weight;
			let tags_weight = 1.0 - weight;

			let mode_label = match (has_text_query, has_image_query) {
				(true, true) => "text+image",
				(true, false) => "text",
				(false, true) => "image",
				(false, false) => unreachable!("validated above"),
			};

			let header_query = match (&text_query, &image) {
				(Some(q), Some(img)) => format!("\"{q}\" + {}", img.display()),
				(Some(q), None) => format!("\"{q}\""),
				(None, Some(img)) => img.display().to_string(),
				(None, None) => unreachable!("validated above"),
			};

			ui_step!(
				"{}",
				format!(
					"{}  ·  mode={}  ·  index image={:.1} tags={:.1}",
					header_query.bright_white().bold(),
					mode_label,
					image_weight,
					tags_weight
				)
				.as_str()
			);

			let query_vec = tokio::task::spawn_blocking({
				let embedder = Arc::clone(&embedder);
				let text_query = text_query.clone();
				let image = image.clone();
				move || -> anyhow::Result<[f32; embed::EMBED_DIM]> {
					match (text_query, image) {
						(Some(text), Some(image_path)) => {
							let image = ImageReader::open(&image_path)?.decode()?;
							let preprocessed = embed::Embedder::preprocess(&image);
							let image_vec = embedder.embed_image(&preprocessed)?;
							let text_vec = embedder.embed_text(&text)?;
							embed::blend_embeddings(&text_vec, &image_vec, weight)
								.map_err(anyhow::Error::from)
						}
						(Some(text), None) => Ok(embedder.embed_text(&text)?),
						(None, Some(image_path)) => {
							let image = ImageReader::open(&image_path)?.decode()?;
							let preprocessed = embed::Embedder::preprocess(&image);
							Ok(embedder.embed_image(&preprocessed)?)
						}
						(None, None) => bail!("provide a text query, --image, or both"),
					}
				}
			})
			.await??;

			let results = store
				.search(
					Some(query_vec.to_vec()),
					Some(query_vec.to_vec()),
					image_weight,
					tags_weight,
					limit,
					site.as_deref(),
				)
				.await?;

			println!();
			if results.is_empty() {
				ui_warn!("No results found");
			} else {
				for r in &results {
					let percent = r.score * 100.0;
					println!(
						"  {}    {}  {}",
						format!("#{}", r.post_id).bright_white().bold(),
						format!("{percent:.2}%").dimmed(),
						r.post_url.cyan(),
					);
				}
				println!();
				ui_success!(
					"{}",
					format!("{} results", results.len().bold().bright_white()).as_str()
				);
			}
		}

		cli::Commands::Cluster {
			qdrant_url,
			site,
			page_size,
			max_points,
			min_cluster_size,
			min_samples,
			allow_single_cluster,
		} => {
			ui::header("roobu · cluster");

			ensure!(page_size > 0, "--page-size must be greater than 0");
			ensure!(max_points > 0, "--max-points must be greater than 0");
			ensure!(
				min_cluster_size >= 2,
				"--min-cluster-size must be at least 2"
			);
			if let Some(ms) = min_samples {
				ensure!(ms >= 1, "--min-samples must be at least 1");
			}

			ui_step!("{}", "Connecting to Qdrant…");
			let store = store::Store::new(&qdrant_url).await?;
			ui_success!("Qdrant ready");

			ui_step!(
				"{}",
				format!(
					"Fetching up to {} vectors (page size {})…",
					max_points, page_size
				)
				.as_str()
			);
			let points = store
				.fetch_image_vectors_for_clustering(site.as_deref(), page_size, max_points)
				.await?;

			if points.is_empty() {
				ui_warn!("No vectors available for clustering");
				return Ok(());
			}

			ensure!(
				points.len() >= min_cluster_size,
				"not enough points ({}) for --min-cluster-size {}",
				points.len(),
				min_cluster_size
			);

			ui_success!("{}", format!("Fetched {} vectors", points.len()).as_str());

			let data: Vec<Vec<f32>> = points.iter().map(|p| p.image_vec.clone()).collect();

			let mut hyper_params = HdbscanHyperParams::builder()
				.min_cluster_size(min_cluster_size)
				.allow_single_cluster(allow_single_cluster);

			if let Some(ms) = min_samples {
				hyper_params = hyper_params.min_samples(ms);
			}

			ui_step!("{}", "Running HDBSCAN…");
			let clusterer = Hdbscan::new(&data, hyper_params.build());
			let labels = clusterer
				.cluster()
				.map_err(|e| anyhow::anyhow!("HDBSCAN failed: {e}"))?;

			let mut cluster_sizes: BTreeMap<i32, usize> = BTreeMap::new();
			for label in &labels {
				*cluster_sizes.entry(*label).or_default() += 1;
			}

			let total = labels.len();
			let noise = cluster_sizes.remove(&-1).unwrap_or(0);
			let cluster_count = cluster_sizes.len();

			println!();
			ui_detail!("samples", "{}", total);
			ui_detail!("clusters", "{}", cluster_count);
			ui_detail!(
				"noise",
				"{} ({:.2}%)",
				noise,
				(noise as f64 * 100.0) / total as f64
			);

			if !cluster_sizes.is_empty() {
				let mut top_clusters: Vec<(i32, usize)> = cluster_sizes.into_iter().collect();
				top_clusters.sort_by(|a, b| b.1.cmp(&a.1));

				println!();
				ui_step!("{}", "Largest clusters:");
				for (label, size) in top_clusters.into_iter().take(10) {
					let ratio = (size as f64 * 100.0) / total as f64;
					println!(
						"  {}  {}  ({ratio:.2}%)",
						format!("cluster {label}").bright_white().bold(),
						format!("{} samples", size).dimmed(),
					);
				}
			}

			println!();
			ui_success!("Clustering complete");
		}
	}

	Ok(())
}

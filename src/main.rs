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

use std::cmp::Ordering;
use std::collections::HashMap;
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
			limit,
			max_cluster_size,
			epsilon,
			allow_single_cluster,
		} => {
			ui::header("roobu · cluster");

			ensure!(page_size > 0, "--page-size must be greater than 0");
			ensure!(max_points > 0, "--max-points must be greater than 0");
			ensure!(limit > 0, "--limit must be greater than 0");
			ensure!(
				min_cluster_size >= 2,
				"--min-cluster-size must be at least 2"
			);
			if let Some(ms) = min_samples {
				ensure!(ms >= 1, "--min-samples must be at least 1");
			}
			if let Some(max_size) = max_cluster_size {
				ensure!(max_size >= 2, "--max-cluster-size must be at least 2");
				ensure!(
					max_size >= min_cluster_size,
					"--max-cluster-size must be greater than or equal to --min-cluster-size"
				);
			}
			if let Some(value) = epsilon {
				ensure!(
					value >= 0.0,
					"--epsilon must be greater than or equal to 0.0"
				);
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
			if let Some(max_size) = max_cluster_size {
				hyper_params = hyper_params.max_cluster_size(max_size);
			}
			if let Some(value) = epsilon {
				hyper_params = hyper_params.epsilon(value);
			}

			ui_step!("{}", "Running HDBSCAN…");
			let labels = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<i32>> {
				let clusterer = Hdbscan::new(&data, hyper_params.build());
				clusterer
					.cluster()
					.map_err(|e| anyhow::anyhow!("HDBSCAN failed: {e}"))
			})
			.await??;

			let total = labels.len();
			let mut cluster_members: HashMap<i32, Vec<usize>> = HashMap::new();
			let mut noise = 0usize;

			for (index, label) in labels.iter().copied().enumerate() {
				if label == -1 {
					noise += 1;
					continue;
				}
				cluster_members.entry(label).or_default().push(index);
			}

			let cluster_count = cluster_members.len();

			println!();
			ui_success!(
				"{} clusters, {} samples, {} noise ({:.2}%)",
				cluster_count,
				total,
				noise,
				(noise as f64 * 100.0) / total as f64
			);

			if cluster_members.is_empty() {
				ui_warn!("All fetched points were classified as noise");
				return Ok(());
			}

			let mut summaries: Vec<ClusterSummary> = cluster_members
				.into_iter()
				.map(|(label, members)| summarize_cluster(label, members, &points))
				.collect();

			summaries.sort_by(|a, b| b.size.cmp(&a.size).then(a.label.cmp(&b.label)));

			if summaries
				.iter()
				.all(|summary| summary.cohesion < config::DEFAULT_CLUSTER_LOW_COHESION_THRESHOLD)
			{
				ui_warn!(
					"Low cohesion detected across all clusters; try higher --min-samples, higher --epsilon, or --max-cluster-size"
				);
			}

			println!();
			ui_step!(
				"{}",
				format!("Cluster previews (up to {} URLs per cluster):", limit).as_str()
			);

			for summary in &summaries {
				let ratio = (summary.size as f64 * 100.0) / total as f64;
				let cohesion_percent = summary.cohesion * 100.0;

				println!(
					"  {}  {}  ({ratio:.2}%)  {}",
					format!("cluster {}", summary.label).bright_white().bold(),
					format!("{} samples", summary.size).dimmed(),
					format!("cohesion {cohesion_percent:.2}%").dimmed(),
				);

				let representative = &points[summary.representative_index];
				println!(
					"    {} {}",
					"representative:".dimmed(),
					cluster_point_label(representative).cyan(),
				);

				let preview_indices: Vec<usize> = summary
					.ranked_indices
					.iter()
					.copied()
					.filter(|index| *index != summary.representative_index)
					.take(limit)
					.collect();

				for (rank, index) in preview_indices.iter().enumerate() {
					println!(
						"    {} {}",
						format!("[{}]", rank + 1).dimmed(),
						cluster_point_label(&points[*index]).cyan(),
					);
				}

				let shown = 1usize + preview_indices.len();
				if summary.size > shown {
					println!(
						"    {}",
						format!("... and {} more", summary.size - shown).dimmed(),
					);
				}

				println!();
			}

			ui_success!("Clustering complete");
		}
	}

	Ok(())
}

struct ClusterSummary {
	label: i32,
	size: usize,
	cohesion: f64,
	representative_index: usize,
	ranked_indices: Vec<usize>,
}

fn summarize_cluster(
	label: i32,
	member_indices: Vec<usize>,
	points: &[store::ClusterPoint],
) -> ClusterSummary {
	let mut centroid = vec![0.0f64; embed::EMBED_DIM];

	for index in &member_indices {
		for (dim, value) in points[*index].image_vec.iter().enumerate() {
			centroid[dim] += f64::from(*value);
		}
	}

	let norm = centroid
		.iter()
		.map(|value| value * value)
		.sum::<f64>()
		.sqrt();
	if norm > 0.0 {
		for value in &mut centroid {
			*value /= norm;
		}
	}

	let mut scored_members: Vec<(usize, f64)> = member_indices
		.iter()
		.map(|index| {
			let similarity = points[*index]
				.image_vec
				.iter()
				.zip(centroid.iter())
				.map(|(lhs, rhs)| f64::from(*lhs) * rhs)
				.sum::<f64>();
			(*index, similarity)
		})
		.collect();

	scored_members.sort_by(|lhs, rhs| {
		rhs.1
			.partial_cmp(&lhs.1)
			.unwrap_or(Ordering::Equal)
			.then(lhs.0.cmp(&rhs.0))
	});

	let cohesion = if scored_members.is_empty() {
		0.0
	} else {
		scored_members
			.iter()
			.map(|(_, similarity)| *similarity)
			.sum::<f64>()
			/ scored_members.len() as f64
	};

	let representative_index = scored_members
		.first()
		.map(|(index, _)| *index)
		.unwrap_or(member_indices[0]);

	let ranked_indices = scored_members.into_iter().map(|(index, _)| index).collect();

	ClusterSummary {
		label,
		size: member_indices.len(),
		cohesion,
		representative_index,
		ranked_indices,
	}
}

fn cluster_point_label(point: &store::ClusterPoint) -> String {
	if point.post_url.is_empty() {
		format!("#{}", point.post_id)
	} else {
		point.post_url.clone()
	}
}

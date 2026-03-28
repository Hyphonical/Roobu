use std::cmp::Ordering;
use std::collections::HashMap;

use anyhow::ensure;
use hdbscan::{Hdbscan, HdbscanHyperParams};
use owo_colors::OwoColorize;

use crate::config;
use crate::embed;
use crate::store;
use crate::ui::{header, ui_step, ui_success, ui_warn};

pub struct Args {
	pub qdrant_url: String,
	pub site: Option<String>,
	pub page_size: u32,
	pub max_points: usize,
	pub min_cluster_size: usize,
	pub min_samples: Option<usize>,
	pub limit: usize,
	pub max_cluster_size: Option<usize>,
	pub epsilon: f64,
	pub allow_single_cluster: bool,
	pub projection_dims: Option<usize>,
	pub projection_nnz: usize,
	pub projection_seed: u64,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · cluster");

	ensure!(args.page_size > 0, "--page-size must be greater than 0");
	ensure!(args.max_points > 0, "--max-points must be greater than 0");
	ensure!(args.limit > 0, "--limit must be greater than 0");
	ensure!(
		args.min_cluster_size >= 2,
		"--min-cluster-size must be at least 2"
	);
	if let Some(ms) = args.min_samples {
		ensure!(ms >= 1, "--min-samples must be at least 1");
	}
	if let Some(max_size) = args.max_cluster_size {
		ensure!(max_size >= 2, "--max-cluster-size must be at least 2");
		ensure!(
			max_size >= args.min_cluster_size,
			"--max-cluster-size must be greater than or equal to --min-cluster-size"
		);
	}
	ensure!(
		args.epsilon >= 0.0,
		"--epsilon must be greater than or equal to 0.0"
	);
	ensure!(
		args.projection_nnz > 0,
		"--projection-nnz must be greater than 0"
	);
	if let Some(dims) = args.projection_dims {
		ensure!(dims >= 2, "--projection-dims must be at least 2");
		ensure!(
			dims <= embed::EMBED_DIM,
			"--projection-dims must be less than or equal to {}",
			embed::EMBED_DIM
		);
	}

	ui_step!("{}", "Connecting to Qdrant…");
	let store = store::Store::new(&args.qdrant_url).await?;
	ui_success!("Qdrant ready");

	ui_step!(
		"{}",
		format!(
			"Fetching up to {} vectors (page size {})…",
			args.max_points, args.page_size
		)
		.as_str()
	);
	let points = store
		.fetch_image_vectors_for_clustering(args.site.as_deref(), args.page_size, args.max_points)
		.await?;

	if points.is_empty() {
		ui_warn!("No vectors available for clustering");
		return Ok(());
	}

	ensure!(
		points.len() >= args.min_cluster_size,
		"not enough points ({}) for --min-cluster-size {}",
		points.len(),
		args.min_cluster_size
	);

	ui_success!("{}", format!("Fetched {} vectors", points.len()).as_str());

	let data = build_cluster_input(
		&points,
		args.projection_dims,
		args.projection_nnz,
		args.projection_seed,
	);

	let mut hyper_params = HdbscanHyperParams::builder()
		.min_cluster_size(args.min_cluster_size)
		.allow_single_cluster(args.allow_single_cluster);

	if let Some(ms) = args.min_samples {
		hyper_params = hyper_params.min_samples(ms);
	}
	if let Some(max_size) = args.max_cluster_size {
		hyper_params = hyper_params.max_cluster_size(max_size);
	}
	hyper_params = hyper_params.epsilon(args.epsilon);

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
		format!("Cluster previews (up to {} URLs per cluster):", args.limit).as_str()
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
			.take(args.limit)
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
	Ok(())
}

fn build_cluster_input(
	points: &[store::ClusterPoint],
	projection_dims: Option<usize>,
	projection_nnz: usize,
	projection_seed: u64,
) -> Vec<Vec<f32>> {
	match projection_dims {
		None => points.iter().map(|point| point.image_vec.clone()).collect(),
		Some(dims) if dims == embed::EMBED_DIM => {
			ui_warn!(
				"--projection-dims equals embedding dimension {}; skipping projection",
				embed::EMBED_DIM
			);
			points.iter().map(|point| point.image_vec.clone()).collect()
		}
		Some(dims) => {
			ui_step!(
				"{}",
				format!(
					"Projecting vectors from {}D to {}D (nnz {}, seed {})…",
					embed::EMBED_DIM,
					dims,
					projection_nnz,
					projection_seed
				)
				.as_str()
			);

			let projection = SparseRandomProjection::new(
				embed::EMBED_DIM,
				dims,
				projection_nnz,
				projection_seed,
			);
			let reduced: Vec<Vec<f32>> = points
				.iter()
				.map(|point| projection.project(&point.image_vec))
				.collect();

			ui_success!("Dimensionality reduction complete");
			reduced
		}
	}
}

struct SparseRandomProjection {
	mapping: Vec<Vec<SparseProjectionEntry>>,
	target_dims: usize,
}

#[derive(Clone, Copy)]
struct SparseProjectionEntry {
	target_index: usize,
	weight: f32,
}

impl SparseRandomProjection {
	fn new(source_dims: usize, target_dims: usize, nnz: usize, seed: u64) -> Self {
		let scale = 1.0f32 / (nnz as f32).sqrt();
		let mut mapping: Vec<Vec<SparseProjectionEntry>> = Vec::with_capacity(source_dims);

		for source_index in 0..source_dims {
			let mut entries = Vec::with_capacity(nnz);
			let mut state = splitmix64(seed ^ source_index as u64);

			for slot in 0..nnz {
				state = splitmix64(state ^ slot as u64);
				let target_index = (state as usize) % target_dims;
				let sign = if state & 1 == 0 { 1.0 } else { -1.0 };

				entries.push(SparseProjectionEntry {
					target_index,
					weight: sign * scale,
				});
			}

			mapping.push(entries);
		}

		Self {
			mapping,
			target_dims,
		}
	}

	fn project(&self, input: &[f32]) -> Vec<f32> {
		let mut output = vec![0.0f32; self.target_dims];

		for (source_index, value) in input.iter().enumerate() {
			if *value == 0.0 || source_index >= self.mapping.len() {
				continue;
			}

			for entry in &self.mapping[source_index] {
				output[entry.target_index] += value * entry.weight;
			}
		}

		let norm = output
			.iter()
			.map(|component| component * component)
			.sum::<f32>()
			.sqrt();

		if norm > 0.0 {
			for component in &mut output {
				*component /= norm;
			}
		}

		output
	}
}

fn splitmix64(mut x: u64) -> u64 {
	x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
	x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
	x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
	x ^ (x >> 31)
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

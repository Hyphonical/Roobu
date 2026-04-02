use std::cmp::Ordering;
use std::collections::HashMap;

use anyhow::ensure;
use owo_colors::OwoColorize;

use crate::cluster::GraphHdbscanParams;
use crate::cluster::graph_hdbscan;
use crate::config;
use crate::embed;
use crate::store;
use crate::ui::header;
use crate::{ui_step, ui_success, ui_warn};

pub struct Args {
	pub qdrant_url: String,
	pub site: Option<String>,
	pub max_points: usize,
	pub min_cluster_size: usize,
	pub projection_dims: usize,
	pub top_clusters: usize,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · cluster");

	ensure!(args.max_points > 0, "--max-points must be greater than 0");
	ensure!(
		args.top_clusters > 0,
		"--top-clusters must be greater than 0"
	);
	ensure!(
		args.min_cluster_size >= 2,
		"--min-cluster-size must be at least 2"
	);
	ensure!(
		args.projection_dims >= 2,
		"--projection-dims must be at least 2"
	);
	ensure!(
		args.projection_dims <= embed::EMBED_DIM,
		"--projection-dims must be less than or equal to {}",
		embed::EMBED_DIM
	);

	ui_step!("{}", "Connecting to Qdrant…");
	let store = store::Store::new(&args.qdrant_url).await?;
	ui_success!("Qdrant ready");

	ui_step!(
		"{}",
		format!(
			"Fetching up to {} vectors (page size {})…",
			args.max_points,
			config::DEFAULT_CLUSTER_PAGE_SIZE
		)
		.as_str()
	);
	let points = store
		.fetch_image_vectors_for_clustering(
			args.site.as_deref(),
			config::DEFAULT_CLUSTER_PAGE_SIZE,
			args.max_points,
		)
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

	let data = build_cluster_input(&points, args.projection_dims);
	let labels = run_cluster(data, &args).await?;

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

	summaries.sort_by(|a, b| {
		b.cohesion
			.partial_cmp(&a.cohesion)
			.unwrap_or(Ordering::Equal)
			.then(b.size.cmp(&a.size))
			.then(a.label.cmp(&b.label))
	});

	if summaries
		.iter()
		.all(|summary| summary.cohesion < config::DEFAULT_CLUSTER_LOW_COHESION_THRESHOLD)
	{
		ui_warn!(
			"Low cohesion detected across clusters; increasing --min-cluster-size can improve precision"
		);
	}

	let shown_clusters = summaries.len().min(args.top_clusters);
	let summaries = summaries
		.into_iter()
		.take(shown_clusters)
		.collect::<Vec<_>>();

	println!();
	ui_step!(
		"{}",
		format!("Top {} most cohesive clusters:", shown_clusters).as_str()
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
			.take(5)
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

async fn run_cluster(data: Vec<Vec<f32>>, args: &Args) -> anyhow::Result<Vec<i32>> {
	ui_step!("Running GraphHDBSCAN with fast defaults…");

	let params = GraphHdbscanParams {
		min_cluster_size: args.min_cluster_size,
		min_samples: args.min_cluster_size,
		epsilon: config::DEFAULT_CLUSTER_GRAPH_EPSILON,
		neighbors: config::DEFAULT_CLUSTER_GRAPH_NEIGHBORS,
		pivots: config::DEFAULT_CLUSTER_GRAPH_PIVOTS,
		top_pivots: config::DEFAULT_CLUSTER_GRAPH_TOP_PIVOTS,
		max_candidates: config::DEFAULT_CLUSTER_GRAPH_MAX_CANDIDATES,
		random_seed: config::DEFAULT_CLUSTER_PROJECTION_SEED,
	};

	tokio::task::spawn_blocking(move || graph_hdbscan::cluster(data, params)).await?
}

fn build_cluster_input(points: &[store::ClusterPoint], projection_dims: usize) -> Vec<Vec<f32>> {
	if projection_dims == embed::EMBED_DIM {
		ui_step!(
			"{}",
			"Projection skipped (already full embedding dimension)"
		);
		return points.iter().map(|point| point.image_vec.clone()).collect();
	}

	ui_step!(
		"{}",
		format!(
			"Projecting vectors from {}D to {}D for faster clustering…",
			embed::EMBED_DIM,
			projection_dims
		)
		.as_str()
	);

	let projection = SparseRandomProjection::new(
		embed::EMBED_DIM,
		projection_dims,
		config::DEFAULT_CLUSTER_PROJECTION_NNZ,
		config::DEFAULT_CLUSTER_PROJECTION_SEED,
	);
	let reduced: Vec<Vec<f32>> = points
		.iter()
		.map(|point| projection.project(&point.image_vec))
		.collect();

	ui_success!("Dimensionality reduction complete");
	reduced
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

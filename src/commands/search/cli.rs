use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, ensure};
use owo_colors::OwoColorize;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::store;
use crate::ui::header;
use crate::{ui_step, ui_success, ui_warn};

use super::service::{SearchRequest, execute_search};

pub struct Args {
	pub query: Option<String>,
	pub image: Option<PathBuf>,
	pub limit: u64,
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub weight: f32,
	pub onnx_optimization: OnnxOptimizationIntensity,
	/// Sites to restrict results to. Empty means all sites.
	pub site: Vec<String>,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · search");

	ensure!(
		(0.0..=1.0).contains(&args.weight),
		"--weight must be between 0.0 and 1.0"
	);

	let text_query = args
		.query
		.as_deref()
		.map(str::trim)
		.filter(|q| !q.is_empty())
		.map(ToOwned::to_owned);
	let has_text_query = text_query.is_some();
	let has_image_query = args.image.is_some();

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
		&args.models_dir,
		model_load,
		args.onnx_optimization,
	)?);
	let store = store::Store::new(&args.qdrant_url).await?;

	let request = SearchRequest {
		text_query,
		image_path: args.image,
		image_bytes: None,
		limit: args.limit,
		site_filter: args.site,
		image_weight: args.weight,
	};

	let response = execute_search(request, &embedder, &store).await?;

	ui_step!(
		"{}",
		format!(
			"{}  ·  mode={}{}",
			response.query_summary.bright_white().bold(),
			response.mode_label,
			response.blend_label.as_deref().unwrap_or_default()
		)
		.as_str()
	);

	println!();
	if response.results.is_empty() {
		ui_warn!("No results found");
		if has_text_query {
			ui_warn!(
				"Text query returned no matches. Ensure text_model_q4f16.onnx and tokenizer.json match the model export used during ingest."
			);
		}
	} else {
		let rows: Vec<(String, String, String, String, String)> = response
			.results
			.iter()
			.map(|r| {
				let id = format!("#{}", r.post_id);
				let percent = format!("{:.2}%", r.score * 100.0);
				let dimensions = if r.width > 0 && r.height > 0 {
					format!("{}x{}", r.width, r.height)
				} else {
					"unknown-size".to_string()
				};
				let ingestion = if r.ingestion_date > 0 {
					format!("ingested={}", r.ingestion_date)
				} else {
					"ingested=unknown".to_string()
				};
				(id, percent, dimensions, ingestion, r.post_url.clone())
			})
			.collect();

		let id_width = rows
			.iter()
			.map(|(id, _, _, _, _)| id.len())
			.max()
			.unwrap_or(0);
		let percent_width = rows
			.iter()
			.map(|(_, percent, _, _, _)| percent.len())
			.max()
			.unwrap_or(0);
		let dimensions_width = rows
			.iter()
			.map(|(_, _, dimensions, _, _)| dimensions.len())
			.max()
			.unwrap_or(0);
		let ingestion_width = rows
			.iter()
			.map(|(_, _, _, ingestion, _)| ingestion.len())
			.max()
			.unwrap_or(0);

		for (id, percent, dimensions, ingestion, url) in rows {
			println!(
				"  {}  {}  {}  {}  {}",
				format!("{id:<id_width$}").bright_white().bold(),
				format!("{percent:>percent_width$}").dimmed(),
				format!("{dimensions:<dimensions_width$}").dimmed(),
				format!("{ingestion:<ingestion_width$}").dimmed(),
				url.cyan()
			);
		}
		println!();
		ui_success!(
			"{}",
			format!("{} results", response.results.len().bold().bright_white()).as_str()
		);
	}

	Ok(())
}

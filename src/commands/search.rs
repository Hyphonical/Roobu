use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use image::DynamicImage;
use owo_colors::OwoColorize;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::store::{self, SearchResult, Store};
use crate::ui::header;
use crate::{ui_step, ui_success, ui_warn};

// ── Application Service (pure logic, reusable by CLI and Web) ────────────────

/// Input parameters for a search operation.
pub struct SearchRequest {
	pub text_query: Option<String>,
	pub image_path: Option<PathBuf>,
	pub limit: u64,
	pub site_filter: Option<String>,
	pub image_weight: f32,
}

/// Result of a search operation with metadata.
pub struct SearchResponse {
	pub results: Vec<SearchResult>,
	pub mode_label: String,
	pub blend_label: Option<String>,
	pub query_summary: String,
}

/// Compute the effective image weight based on query mode and requested weight.
pub fn effective_image_weight(
	has_text_query: bool,
	has_image_query: bool,
	requested_weight: f32,
) -> f32 {
	match (has_text_query, has_image_query) {
		(true, false) => 0.0,
		(false, true) => 1.0,
		_ => requested_weight,
	}
}

/// Generate a blend label for hybrid search modes.
pub fn blend_label(image_weight: f32) -> Option<String> {
	if image_weight > 0.0 && image_weight < 1.0 {
		Some(format!(
			"  ·  blend image={:.1} text={:.1}",
			image_weight,
			1.0 - image_weight
		))
	} else {
		None
	}
}

fn load_query_image(path: &std::path::Path) -> anyhow::Result<DynamicImage> {
	let bytes = std::fs::read(path)
		.with_context(|| format!("Failed to read image file {}", path.display()))?;

	image::load_from_memory(&bytes).with_context(|| {
		format!(
			"Failed to decode image {} from file content",
			path.display()
		)
	})
}

/// Execute a search operation. This is the pure application logic that can be
/// reused by both CLI and web handlers.
pub async fn execute_search(
	request: SearchRequest,
	embedder: &Arc<embed::Embedder>,
	store: &Store,
) -> anyhow::Result<SearchResponse> {
	let has_text_query = request.text_query.is_some();
	let has_image_query = request.image_path.is_some();

	if !has_text_query && !has_image_query {
		bail!("provide a text query, --image, or both");
	}

	let query_image_weight =
		effective_image_weight(has_text_query, has_image_query, request.image_weight);

	let mode_label = match (has_text_query, has_image_query) {
		(true, true) => "text+image",
		(true, false) => "text",
		(false, true) => "image",
		(false, false) => unreachable!("validated above"),
	};

	let header_query = match (&request.text_query, &request.image_path) {
		(Some(q), Some(img)) => format!("\"{q}\" + {}", img.display()),
		(Some(q), None) => format!("\"{q}\""),
		(None, Some(img)) => img.display().to_string(),
		(None, None) => unreachable!("validated above"),
	};

	let query_vec = tokio::task::spawn_blocking({
		let embedder = Arc::clone(embedder);
		let text_query = request.text_query.clone();
		let image = request.image_path.clone();
		let image_weight = query_image_weight;

		move || -> anyhow::Result<[f32; embed::EMBED_DIM]> {
			match (text_query, image) {
				(Some(text), Some(image_path)) => {
					let image = load_query_image(&image_path)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					let image_vec = embedder.embed_image(&preprocessed)?;
					let text_vec = embedder.embed_text(&text)?;
					embed::blend_embeddings(&text_vec, &image_vec, image_weight)
						.map_err(anyhow::Error::from)
				}
				(Some(text), None) => Ok(embedder.embed_text(&text)?),
				(None, Some(image_path)) => {
					let image = load_query_image(&image_path)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					Ok(embedder.embed_image(&preprocessed)?)
				}
				(None, None) => bail!("provide a text query, --image, or both"),
			}
		}
	})
	.await??;

	ensure!(
		query_vec.iter().all(|value| value.is_finite()),
		"generated query embedding contains non-finite values; verify text_model_q4f16.onnx and tokenizer.json match the indexed model set"
	);
	let query_norm_sq: f32 = query_vec.iter().map(|value| value * value).sum();
	ensure!(
		query_norm_sq > 1e-6,
		"generated query embedding is near zero; verify text_model_q4f16.onnx and tokenizer.json are valid and aligned with ingest"
	);

	let results = store
		.search(
			query_vec.to_vec(),
			request.limit,
			request.site_filter.as_deref(),
		)
		.await?;

	Ok(SearchResponse {
		results,
		mode_label: mode_label.to_string(),
		blend_label: blend_label(query_image_weight),
		query_summary: header_query,
	})
}

// ── CLI Adapter (terminal presentation only) ────────────────────────────────

pub struct Args {
	pub query: Option<String>,
	pub image: Option<PathBuf>,
	pub limit: u64,
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub weight: f32,
	pub onnx_optimization: OnnxOptimizationIntensity,
	pub site: Option<String>,
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

#[cfg(test)]
mod tests {
	use super::{blend_label, effective_image_weight};

	#[test]
	fn effective_image_weight_is_text_only_for_text_mode() {
		assert_eq!(effective_image_weight(true, false, 1.0), 0.0);
	}

	#[test]
	fn effective_image_weight_is_image_only_for_image_mode() {
		assert_eq!(effective_image_weight(false, true, 0.2), 1.0);
	}

	#[test]
	fn effective_image_weight_uses_requested_value_for_hybrid_mode() {
		assert_eq!(effective_image_weight(true, true, 0.6), 0.6);
	}

	#[test]
	fn blend_label_is_hidden_for_pure_text_or_image_weights() {
		assert!(blend_label(0.0).is_none());
		assert!(blend_label(1.0).is_none());
	}

	#[test]
	fn blend_label_is_shown_for_mixed_weights() {
		assert_eq!(
			blend_label(0.4).as_deref(),
			Some("  ·  blend image=0.4 text=0.6")
		);
	}
}

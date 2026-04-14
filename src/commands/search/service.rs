use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use image::DynamicImage;

use crate::embed;
use crate::store::{SearchResult, Store};

/// Input parameters for a search operation.
pub struct SearchRequest {
	pub text_query: Option<String>,
	pub image_path: Option<PathBuf>,
	pub image_bytes: Option<Vec<u8>>,
	pub limit: u64,
	/// Sites to restrict results to. Empty means all sites.
	pub site_filter: Vec<String>,
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

fn load_query_image_from_bytes(bytes: &[u8]) -> anyhow::Result<DynamicImage> {
	image::load_from_memory(bytes).with_context(|| "Failed to decode image from downloaded content")
}

/// Execute a search operation. This is the pure application logic that can be
/// reused by both CLI and web handlers.
pub async fn execute_search(
	request: SearchRequest,
	embedder: &Arc<embed::Embedder>,
	store: &Store,
) -> anyhow::Result<SearchResponse> {
	let has_text_query = request.text_query.is_some();
	let has_image_query = request.image_path.is_some() || request.image_bytes.is_some();

	ensure!(
		!(request.image_path.is_some() && request.image_bytes.is_some()),
		"provide either an image path or in-memory image bytes, not both"
	);

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

	let header_query = match (
		&request.text_query,
		&request.image_path,
		request.image_bytes.as_ref(),
	) {
		(Some(q), Some(img), None) => format!("\"{q}\" + {}", img.display()),
		(Some(q), None, Some(_)) => format!("\"{q}\" + [remote image]"),
		(Some(q), None, None) => format!("\"{q}\""),
		(None, Some(img), None) => img.display().to_string(),
		(None, None, Some(_)) => "[remote image]".to_string(),
		(None, None, None) => unreachable!("validated above"),
		_ => unreachable!("validated by input source check"),
	};

	let query_vec = tokio::task::spawn_blocking({
		let embedder = Arc::clone(embedder);
		let text_query = request.text_query.clone();
		let image_path = request.image_path.clone();
		let image_bytes = request.image_bytes.clone();
		let image_weight = query_image_weight;

		move || -> anyhow::Result<[f32; embed::EMBED_DIM]> {
			match (text_query, image_path, image_bytes) {
				(Some(text), Some(image_path), None) => {
					let image = load_query_image(&image_path)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					let image_vec = embedder.embed_image(&preprocessed)?;
					let text_vec = embedder.embed_text(&text)?;
					embed::blend_embeddings(&text_vec, &image_vec, image_weight)
						.map_err(anyhow::Error::from)
				}
				(Some(text), None, Some(image_bytes)) => {
					let image = load_query_image_from_bytes(&image_bytes)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					let image_vec = embedder.embed_image(&preprocessed)?;
					let text_vec = embedder.embed_text(&text)?;
					embed::blend_embeddings(&text_vec, &image_vec, image_weight)
						.map_err(anyhow::Error::from)
				}
				(Some(text), None, None) => Ok(embedder.embed_text(&text)?),
				(None, Some(image_path), None) => {
					let image = load_query_image(&image_path)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					Ok(embedder.embed_image(&preprocessed)?)
				}
				(None, None, Some(image_bytes)) => {
					let image = load_query_image_from_bytes(&image_bytes)?;
					let preprocessed = embed::Embedder::preprocess(&image);
					Ok(embedder.embed_image(&preprocessed)?)
				}
				(None, None, None) => bail!("provide a text query, --image, or both"),
				_ => bail!("provide either an image path or in-memory image bytes, not both"),
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

	let site_refs: Vec<&str> = request.site_filter.iter().map(String::as_str).collect();
	let results = store
		.search(query_vec.to_vec(), request.limit, &site_refs)
		.await?;

	Ok(SearchResponse {
		results,
		mode_label: mode_label.to_string(),
		blend_label: blend_label(query_image_weight),
		query_summary: header_query,
	})
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

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, ensure};
use image::ImageReader;
use owo_colors::OwoColorize;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::store;
use crate::ui::{header, ui_step, ui_success, ui_warn};

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

	let image_weight = args.weight;

	let mode_label = match (has_text_query, has_image_query) {
		(true, true) => "text+image",
		(true, false) => "text",
		(false, true) => "image",
		(false, false) => unreachable!("validated above"),
	};

	let header_query = match (&text_query, &args.image) {
		(Some(q), Some(img)) => format!("\"{q}\" + {}", img.display()),
		(Some(q), None) => format!("\"{q}\""),
		(None, Some(img)) => img.display().to_string(),
		(None, None) => unreachable!("validated above"),
	};

	ui_step!(
		"{}",
		format!(
			"{}  ·  mode={}  ·  blend image={:.1} text={:.1}",
			header_query.bright_white().bold(),
			mode_label,
			image_weight,
			1.0 - image_weight
		)
		.as_str()
	);

	let query_vec = tokio::task::spawn_blocking({
		let embedder = Arc::clone(&embedder);
		let text_query = text_query.clone();
		let image = args.image.clone();
		let image_weight = args.weight;

		move || -> anyhow::Result<[f32; embed::EMBED_DIM]> {
			match (text_query, image) {
				(Some(text), Some(image_path)) => {
					let image = ImageReader::open(&image_path)?.decode()?;
					let preprocessed = embed::Embedder::preprocess(&image);
					let image_vec = embedder.embed_image(&preprocessed)?;
					let text_vec = embedder.embed_text(&text)?;
					embed::blend_embeddings(&text_vec, &image_vec, image_weight)
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
		.search(query_vec.to_vec(), args.limit, args.site.as_deref())
		.await?;

	println!();
	if results.is_empty() {
		ui_warn!("No results found");
	} else {
		for r in &results {
			let percent = r.score * 100.0;
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
			println!(
				"  {}    {}  {}  {}  {}",
				format!("#{}", r.post_id).bright_white().bold(),
				format!("{percent:.2}%").dimmed(),
				dimensions.dimmed(),
				ingestion.dimmed(),
				r.post_url.cyan()
			);
		}
		println!();
		ui_success!(
			"{}",
			format!("{} results", results.len().bold().bright_white()).as_str()
		);
	}

	Ok(())
}

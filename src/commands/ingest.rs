use std::path::PathBuf;
use std::sync::Arc;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::ingest;
use crate::sites;
use crate::store;
use crate::ui::{header, ui_step, ui_success};

pub struct Args {
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub checkpoint: PathBuf,
	pub poll_interval: u64,
	pub batch_size: usize,
	pub download_concurrency: usize,
	pub api_key: String,
	pub user_id: String,
	pub onnx_optimization: OnnxOptimizationIntensity,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · init");

	ui_step!("{}", "Loading embedder…");
	let embedder = Arc::new(embed::Embedder::new(
		&args.models_dir,
		embed::ModelLoad::TextAndVision,
		args.onnx_optimization,
	)?);
	ui_success!("Embedder ready");

	ui_step!("{}", "Connecting to Qdrant…");
	let store = store::Store::new(&args.qdrant_url).await?;
	ui_success!("Qdrant ready");

	let client = sites::rule34::Rule34Client::new(args.api_key, args.user_id)?;

	let ingest_config = ingest::IngestConfig {
		poll_interval_secs: args.poll_interval,
		batch_size: args.batch_size,
		download_concurrency: args.download_concurrency,
	};

	ingest::run(client, &store, embedder, &args.checkpoint, &ingest_config).await
}

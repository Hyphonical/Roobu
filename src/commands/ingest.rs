use std::path::PathBuf;
use std::sync::Arc;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::ingest;
use crate::sites;
use crate::store;
use crate::ui::{header, ui_step, ui_success};

pub struct Args {
	pub site: sites::SiteKind,
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub checkpoint: PathBuf,
	pub poll_interval: u64,
	pub batch_size: usize,
	pub download_concurrency: usize,
	pub rule34_api_key: Option<String>,
	pub rule34_user_id: Option<String>,
	pub e621_login: Option<String>,
	pub e621_api_key: Option<String>,
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

	let ingest_config = ingest::IngestConfig {
		poll_interval_secs: args.poll_interval,
		batch_size: args.batch_size,
		download_concurrency: args.download_concurrency,
	};

	let client = sites::build_client(
		args.site,
		sites::SiteCredentials {
			rule34_api_key: args.rule34_api_key,
			rule34_user_id: args.rule34_user_id,
			e621_login: args.e621_login,
			e621_api_key: args.e621_api_key,
		},
	)?;

	ingest::run(client, &store, embedder, &args.checkpoint, &ingest_config).await
}

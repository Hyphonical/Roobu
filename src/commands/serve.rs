//! Serve command — starts the web API server.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::store;
use crate::ui::header;
use crate::web;
use crate::{ui_step, ui_success};

/// CLI arguments for the serve command.
pub struct Args {
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub address: String,
	pub onnx_optimization: OnnxOptimizationIntensity,
}

/// Start the web API server with the given configuration.
pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · serve");

	ui_step!("Loading embedder…");
	let embedder = Arc::new(
		embed::Embedder::new(
			&args.models_dir,
			embed::ModelLoad::VisionOnly,
			args.onnx_optimization,
		)
		.with_context(|| "failed to load embedder")?,
	);
	ui_success!("Embedder ready");

	ui_step!("Connecting to Qdrant…");
	let store = store::Store::new(&args.qdrant_url)
		.await
		.with_context(|| "failed to connect to Qdrant")?;
	ui_success!("Qdrant ready");

	let app = web::create_router(store, embedder);

	let listener = tokio::net::TcpListener::bind(&args.address)
		.await
		.with_context(|| format!("failed to bind to {}", args.address))?;

	ui_success!("Web server listening on http://{}", args.address);

	axum::serve(listener, app).await?;

	Ok(())
}

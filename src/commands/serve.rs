//! Serve command — starts the web API server.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::ingest::{self, IngestEvent};
use crate::store;
use crate::ui::header;
use crate::web;
use crate::{ui_step, ui_success};

/// CLI arguments for the serve command.
pub struct Args {
	pub qdrant_url: String,
	pub models_dir: PathBuf,
	pub checkpoint: PathBuf,
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
			embed::ModelLoad::TextAndVision,
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

	let state = web::create_state(store, embedder);

	let monitor_state = state.clone();
	let monitor_checkpoint = args.checkpoint.clone();
	tokio::spawn(async move {
		monitor_checkpoint_file(monitor_checkpoint, monitor_state).await;
	});

	let app = web::create_router(state);

	let listener = tokio::net::TcpListener::bind(&args.address)
		.await
		.with_context(|| format!("failed to bind to {}", args.address))?;

	ui_success!("Web server listening on http://{}", args.address);

	axum::serve(listener, app).await?;

	Ok(())
}

async fn monitor_checkpoint_file(checkpoint_path: PathBuf, state: web::AppState) {
	let mut previous = ingest::checkpoint_load(&checkpoint_path);
	publish_checkpoint_snapshot(&state, &previous).await;

	loop {
		tokio::time::sleep(Duration::from_secs(3)).await;

		let current = ingest::checkpoint_load(&checkpoint_path);
		if current == previous {
			continue;
		}

		for (site, last_id) in &current {
			let old = previous.get(site).copied();
			if old != Some(*last_id) {
				state
					.publish_ingest_event(IngestEvent::CheckpointUpdated {
						site: site.clone(),
						last_id: *last_id,
					})
					.await;
			}
		}

		publish_checkpoint_snapshot(&state, &current).await;
		previous = current;
	}
}

async fn publish_checkpoint_snapshot(state: &web::AppState, map: &ingest::CheckpointMap) {
	let mut active_sites: Vec<String> = map.keys().cloned().collect();
	active_sites.sort();

	let last_checkpoint = if map.is_empty() {
		None
	} else {
		serde_json::to_value(map).ok()
	};

	state
		.publish_ingest_event(IngestEvent::StatusSnapshot {
			is_running: !map.is_empty(),
			active_sites,
			last_checkpoint,
		})
		.await;
}

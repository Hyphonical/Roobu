//! Ingest pipeline for fetching, downloading, embedding, and upserting posts.
//!
//! The pipeline processes posts in stages:
//! 1. Fetch recent posts from a site API
//! 2. Download and validate thumbnails concurrently
//! 3. Embed valid images through the ONNX vision model
//! 4. Upsert embeddings into Qdrant with metadata

pub mod checkpoint;
mod cycle;

pub use checkpoint::{CheckpointMap, get as checkpoint_get, load as checkpoint_load};
pub use cycle::{IngestConfig, run_cycle};

use crate::sites::BooruClient;
use owo_colors::OwoColorize;

/// Run the ingest loop for a single site.
///
/// Fetches new posts, downloads thumbnails, embeds them, and upserts into
/// Qdrant. Repeats indefinitely with a configurable poll interval.
pub async fn run(
	client: impl crate::sites::BooruClient,
	store: &crate::store::Store,
	embedder: std::sync::Arc<crate::embed::Embedder>,
	checkpoint_path: &std::path::Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	let site = client.site_name();
	crate::ui::header(&format!("ingest · {site}"));

	let mut ckpt: CheckpointMap = checkpoint_load(checkpoint_path);
	let mut last_id = checkpoint_get(&ckpt, site);

	crate::ui_step!("Resuming from post {}", last_id.bold().bright_white());

	let mut context = cycle::CycleContext {
		store,
		embedder,
		checkpoint_path,
		config,
		ckpt: &mut ckpt,
	};

	loop {
		let cycle_start = std::time::Instant::now();
		match run_cycle(&client, site, &mut last_id, &mut context).await {
			Ok(stats) => cycle::print_cycle_stats(&stats),
			Err(error) => {
				let elapsed = cycle_start.elapsed();
				let error_chain = format!("{error:#}");
				crate::ui_warn!(
					"{} cycle failed ({error_chain}) after {} · skipping until next poll",
					site,
					cycle::format_elapsed(elapsed)
				);
				tracing::warn!(
					site,
					error = %error,
					error_chain = %error_chain,
					elapsed_secs = elapsed.as_secs_f64(),
					"site ingest cycle failed; continuing"
				);
			}
		}

		tracing::debug!(
			site,
			sleep_secs = config.poll_interval_secs,
			"ingest loop sleep"
		);

		tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)).await;
	}
}

/// Run the ingest loop for multiple sites sequentially.
///
/// Cycles through all configured sites, fetching and embedding new posts
/// from each. Maintains per-site checkpoint state and adjusts sleep
/// duration to keep the overall cadence close to the target interval.
pub async fn run_multi(
	clients: Vec<crate::sites::SiteClient>,
	store: &crate::store::Store,
	embedder: std::sync::Arc<crate::embed::Embedder>,
	checkpoint_path: &std::path::Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	use owo_colors::OwoColorize;
	use std::time::{Duration, Instant};

	if clients.is_empty() {
		anyhow::bail!("no ingest clients configured");
	}

	let mut ckpt = checkpoint_load(checkpoint_path);
	let mut states: Vec<cycle::SiteLoopState> = clients
		.into_iter()
		.map(|client| {
			let site = client.site_name();
			let last_id = checkpoint_get(&ckpt, site);
			cycle::SiteLoopState {
				client,
				site,
				last_id,
				resume_announced: false,
			}
		})
		.collect();

	let mut context = cycle::CycleContext {
		store,
		embedder,
		checkpoint_path,
		config,
		ckpt: &mut ckpt,
	};

	loop {
		// Keep per-site cadence close to the target by subtracting ingest runtime from sleep.
		let site_count = states.len().max(1) as f64;
		let per_site_target =
			Duration::from_secs_f64((config.poll_interval_secs as f64 / site_count).max(1.0));

		for state in &mut states {
			crate::ui::header(&format!("ingest · {}", state.site));
			if !state.resume_announced {
				crate::ui_step!("Resuming from post {}", state.last_id.bold().bright_white());
				state.resume_announced = true;
			}

			let cycle_start = Instant::now();
			match run_cycle(&state.client, state.site, &mut state.last_id, &mut context).await {
				Ok(cycle_stats) => cycle::print_cycle_stats(&cycle_stats),
				Err(error) => {
					let elapsed = cycle_start.elapsed();
					let error_chain = format!("{error:#}");
					crate::ui_warn!(
						"{} cycle failed ({error_chain}) after {} · skipping this site for now",
						state.site,
						cycle::format_elapsed(elapsed)
					);
					tracing::warn!(
						site = state.site,
						error = %error,
						error_chain = %error_chain,
						elapsed_secs = elapsed.as_secs_f64(),
						"site ingest cycle failed in all-sites mode; continuing"
					);
				}
			}

			let elapsed = cycle_start.elapsed();
			let sleep_duration = if elapsed >= per_site_target {
				Duration::from_secs(1)
			} else {
				per_site_target - elapsed
			};

			tracing::debug!(
				site = state.site,
				target_secs = per_site_target.as_secs_f64(),
				elapsed_secs = elapsed.as_secs_f64(),
				sleep_secs = sleep_duration.as_secs_f64(),
				"per-site ingest loop sleep"
			);
			tokio::time::sleep(sleep_duration).await;
		}
	}
}

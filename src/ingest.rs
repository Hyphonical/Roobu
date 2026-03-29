use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use futures::stream::{self, StreamExt};
use image::DynamicImage;
use owo_colors::OwoColorize;
use tokio::sync::{Semaphore, mpsc};

use crate::checkpoint::{self, CheckpointMap};
use crate::embed::Embedder;
use crate::error::RoobuError;
use crate::sites::{BooruClient, Post, SiteClient, validate_downloaded_image};
use crate::store::{PostEmbedding, Store};
use crate::ui::*;

pub struct IngestConfig {
	pub poll_interval_secs: u64,
	pub batch_size: usize,
	pub download_concurrency: usize,
	pub site_fetch_timeout_secs: u64,
}

struct SiteLoopState {
	client: SiteClient,
	site: &'static str,
	last_id: u64,
	resume_announced: bool,
}

struct CycleContext<'a> {
	store: &'a Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &'a Path,
	config: &'a IngestConfig,
	ckpt: &'a mut CheckpointMap,
}

struct DownloadedBatch {
	batch_len: usize,
	downloaded: Vec<(Post, DynamicImage)>,
}

const DOWNLOAD_QUEUE_MAX_DEPTH: usize = 4;

async fn download_batch(
	client: &impl BooruClient,
	batch: Vec<Post>,
	download_concurrency: usize,
) -> DownloadedBatch {
	let batch_len = batch.len();
	let semaphore = Arc::new(Semaphore::new(download_concurrency));

	let downloaded: Vec<(Post, DynamicImage)> = stream::iter(batch)
		.map(|post| {
			let sem = semaphore.clone();
			async move {
				let _permit = sem.acquire().await.unwrap();
				let url = post.thumbnail_url.clone();
				match client.download_thumbnail(&url).await {
					Ok(data) => validate_downloaded_image(post.id, &data).map(|img| (post, img)),
					Err(e) => {
						tracing::warn!(post_id = post.id, error = %e, "download failed");
						None
					}
				}
			}
		})
		.buffer_unordered(download_concurrency)
		.filter_map(|x| async { x })
		.collect()
		.await;

	DownloadedBatch {
		batch_len,
		downloaded,
	}
}

async fn process_downloaded_batch(
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
	batch: DownloadedBatch,
) -> anyhow::Result<()> {
	let DownloadedBatch {
		batch_len,
		downloaded,
	} = batch;

	if downloaded.is_empty() {
		ui_warn!("batch had no valid images after download");
		return Ok(());
	}

	let valid_count = downloaded.len();
	let skipped = batch_len - valid_count;

	ui_detail!(
		"Valid",
		"{}",
		format!(
			"{} images{}",
			valid_count.bold().bright_white(),
			if skipped > 0 {
				format!("  ·  {} skipped", skipped)
			} else {
				String::new()
			}
		)
		.as_str()
	);

	let posts_for_embed: Vec<Post> = downloaded.iter().map(|(p, _)| p.clone()).collect();
	let images: Vec<DynamicImage> = downloaded.into_iter().map(|(_, img)| img).collect();

	let embedder_clone = context.embedder.clone();
	let new_last = posts_for_embed
		.iter()
		.map(|p| p.id)
		.max()
		.unwrap_or(*last_id);
	let embeddings =
		tokio::task::spawn_blocking(move || -> Result<Vec<PostEmbedding>, RoobuError> {
			let preprocessed: Vec<DynamicImage> = images.iter().map(Embedder::preprocess).collect();

			let image_vecs = embedder_clone.embed_images(&preprocessed)?;
			let tag_texts: Vec<String> = posts_for_embed
				.iter()
				.map(|post| post.tags_normalized())
				.collect();
			let tags_vecs = embedder_clone.embed_texts(&tag_texts)?;

			let mut results = Vec::with_capacity(posts_for_embed.len());
			for ((post, image_vec), tags_vec) in posts_for_embed
				.into_iter()
				.zip(image_vecs.into_iter())
				.zip(tags_vecs.into_iter())
			{
				results.push(PostEmbedding {
					post_id: post.id,
					site: post.site,
					site_namespace: post.site_namespace,
					post_url: post.post_url(),
					thumbnail_url: post.thumbnail_url.clone(),
					rating: post.rating.clone(),
					image_vec,
					tags_vec,
				});
			}
			Ok(results)
		})
		.await??;

	context.store.upsert(embeddings).await?;

	if new_last > *last_id {
		*last_id = new_last;
		checkpoint::set(context.ckpt, site, *last_id);
		checkpoint::save(context.checkpoint_path, context.ckpt)?;
	}

	ui_success!(
		"{}",
		format!(
			"Upserted {} posts  ·  checkpoint {}",
			valid_count.bold().bright_white(),
			(*last_id).bold().bright_white()
		)
		.as_str()
	);

	Ok(())
}

async fn run_cycle(
	client: &impl BooruClient,
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
) -> anyhow::Result<()> {
	let fetch_timeout = Duration::from_secs(context.config.site_fetch_timeout_secs.max(1));
	let posts = match tokio::time::timeout(fetch_timeout, client.fetch_recent(*last_id)).await {
		Ok(fetch_result) => {
			fetch_result.with_context(|| format!("{site}: failed to fetch recent posts"))?
		}
		Err(_) => {
			anyhow::bail!("{site}: fetch timed out after {}s", fetch_timeout.as_secs())
		}
	};
	let posts: Vec<Post> = posts.into_iter().filter(|p| p.passes_preflight()).collect();

	if posts.is_empty() {
		ui_step!("{}", "No new posts");
		tracing::debug!(site, "no new posts");
		return Ok(());
	}

	ui_step!(
		"{}",
		format!("Fetched {} new posts", posts.len().bold().bright_white()).as_str()
	);

	let batch_size = context.config.batch_size;
	let download_concurrency = context.config.download_concurrency;
	let queue_depth = (posts.len().div_ceil(batch_size)).clamp(1, DOWNLOAD_QUEUE_MAX_DEPTH);
	let (tx, mut rx) = mpsc::channel::<DownloadedBatch>(queue_depth);

	// Move sender ownership into the producer so channel closure is observable by the consumer.
	let producer = async move {
		for batch in posts.chunks(batch_size).map(|chunk| chunk.to_vec()) {
			let downloaded = download_batch(client, batch, download_concurrency).await;
			if tx.send(downloaded).await.is_err() {
				break;
			}
		}
		Ok::<(), anyhow::Error>(())
	};

	let consumer = async {
		while let Some(downloaded) = rx.recv().await {
			process_downloaded_batch(site, last_id, context, downloaded).await?;
		}
		Ok::<(), anyhow::Error>(())
	};

	tokio::try_join!(producer, consumer)?;

	Ok(())
}

pub async fn run(
	client: impl BooruClient,
	store: &Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	let site = client.site_name();
	header(&format!("ingest · {site}"));

	let mut ckpt: CheckpointMap = checkpoint::load(checkpoint_path);
	let mut last_id = checkpoint::get(&ckpt, site);

	ui_step!(
		"{}",
		format!("Resuming from post {}", last_id.bold().bright_white()).as_str()
	);

	let mut context = CycleContext {
		store,
		embedder,
		checkpoint_path,
		config,
		ckpt: &mut ckpt,
	};

	loop {
		if let Err(error) = run_cycle(&client, site, &mut last_id, &mut context).await {
			let error_chain = format!("{error:#}");
			ui_warn!(
				"{}",
				format!("{site} cycle failed ({error_chain}) · skipping until next poll").as_str()
			);
			tracing::warn!(
				site,
				error = %error,
				error_chain = %error_chain,
				"site ingest cycle failed; continuing"
			);
		}

		tracing::debug!(
			site,
			sleep_secs = config.poll_interval_secs,
			"ingest loop sleep"
		);

		tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)).await;
	}
}

pub async fn run_multi(
	clients: Vec<SiteClient>,
	store: &Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	if clients.is_empty() {
		anyhow::bail!("no ingest clients configured");
	}

	let mut ckpt = checkpoint::load(checkpoint_path);
	let mut states: Vec<SiteLoopState> = clients
		.into_iter()
		.map(|client| {
			let site = client.site_name();
			let last_id = checkpoint::get(&ckpt, site);
			SiteLoopState {
				client,
				site,
				last_id,
				resume_announced: false,
			}
		})
		.collect();

	let mut context = CycleContext {
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
			header(&format!("ingest · {}", state.site));
			if !state.resume_announced {
				ui_step!(
					"{}",
					format!("Resuming from post {}", state.last_id.bold().bright_white()).as_str()
				);
				state.resume_announced = true;
			}

			let cycle_start = Instant::now();
			if let Err(error) =
				run_cycle(&state.client, state.site, &mut state.last_id, &mut context).await
			{
				let error_chain = format!("{error:#}");
				ui_warn!(
					"{}",
					format!(
						"{} cycle failed ({error_chain}) · skipping this site for now",
						state.site
					)
					.as_str()
				);
				tracing::warn!(
					site = state.site,
					error = %error,
					error_chain = %error_chain,
					"site ingest cycle failed in all-sites mode; continuing"
				);
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

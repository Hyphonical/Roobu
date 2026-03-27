use std::path::Path;
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use image::DynamicImage;
use owo_colors::OwoColorize;
use tokio::sync::Semaphore;

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
}

struct SiteLoopState {
	client: SiteClient,
	site: &'static str,
	last_id: u64,
}

struct CycleContext<'a> {
	store: &'a Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &'a Path,
	config: &'a IngestConfig,
	ckpt: &'a mut CheckpointMap,
}

async fn run_cycle(
	client: &impl BooruClient,
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
) -> anyhow::Result<()> {
	let posts = client.fetch_recent(*last_id).await?;
	let posts: Vec<Post> = posts.into_iter().filter(|p| p.passes_preflight()).collect();

	if posts.is_empty() {
		tracing::debug!(site, "no new posts");
		return Ok(());
	}

	ui_step!(
		"{}",
		format!("Fetched {} new posts", posts.len().bold().bright_white()).as_str()
	);

	for batch in posts.chunks(context.config.batch_size) {
		let batch = batch.to_vec();
		let batch_len = batch.len();

		let semaphore = Arc::new(Semaphore::new(context.config.download_concurrency));
		let http_client = client;

		let downloaded: Vec<(Post, DynamicImage)> = stream::iter(batch)
			.map(|post| {
				let sem = semaphore.clone();
				async move {
					let _permit = sem.acquire().await.unwrap();
					let url = post.preview_url.clone();
					match http_client.download_preview(&url).await {
						Ok(data) => {
							validate_downloaded_image(post.id, &data).map(|img| (post, img))
						}
						Err(e) => {
							tracing::warn!(post_id = post.id, error = %e, "download failed");
							None
						}
					}
				}
			})
			.buffer_unordered(context.config.download_concurrency)
			.filter_map(|x| async { x })
			.collect()
			.await;

		if downloaded.is_empty() {
			ui_warn!("batch had no valid images after download");
			continue;
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
				let preprocessed: Vec<DynamicImage> =
					images.iter().map(Embedder::preprocess).collect();

				let image_vecs = embedder_clone.embed_images(&preprocessed)?;

				let mut results = Vec::with_capacity(posts_for_embed.len());
				for (i, post) in posts_for_embed.iter().enumerate() {
					let tags_vec = embedder_clone.embed_text(&post.tags_normalized())?;
					results.push(PostEmbedding {
						post_id: post.id,
						site: post.site,
						site_namespace: post.site_namespace,
						post_url: post.post_url(),
						rating: post.rating.clone(),
						image_vec: image_vecs[i],
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
	}

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
		run_cycle(&client, site, &mut last_id, &mut context).await?;

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
			header(&format!("ingest · {site}"));
			ui_step!(
				"{}",
				format!("Resuming from post {}", last_id.bold().bright_white()).as_str()
			);
			SiteLoopState {
				client,
				site,
				last_id,
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
    // Compute per-site sleep duration (rounded down). Ensure at least 1 second.
    let per_site_sleep = if !states.is_empty() {
        config.poll_interval_secs / states.len() as u64
    } else {
        config.poll_interval_secs
    };
    let per_site_sleep = per_site_sleep.max(1);
    for state in &mut states {
        run_cycle(&state.client, state.site, &mut state.last_id, &mut context).await?;
        tracing::debug!(
            site = state.site,
            sleep_secs = per_site_sleep,
            "per-site ingest loop sleep"
        );
        tokio::time::sleep(std::time::Duration::from_secs(per_site_sleep)).await;
    }
}
}

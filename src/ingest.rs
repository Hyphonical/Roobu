use std::path::Path;
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use image::DynamicImage;
use owo_colors::OwoColorize;
use tokio::sync::Semaphore;

use crate::checkpoint::{self, CheckpointMap};
use crate::embed::Embedder;
use crate::error::RoobuError;
use crate::sites::{BooruClient, Post, validate_downloaded_image};
use crate::store::{PostEmbedding, Store};
use crate::ui::*;

pub struct IngestConfig {
	pub poll_interval_secs: u64,
	pub batch_size: usize,
	pub download_concurrency: usize,
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

	loop {
		let posts = client.fetch_recent(last_id).await?;
		let posts: Vec<Post> = posts.into_iter().filter(|p| p.passes_preflight()).collect();

		if posts.is_empty() {
			tracing::debug!("no new posts, sleeping {}s", config.poll_interval_secs);
			tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)).await;
			continue;
		}

		ui_step!(
			"{}",
			format!("Fetched {} new posts", posts.len().bold().bright_white()).as_str()
		);

		for batch in posts.chunks(config.batch_size) {
			let batch = batch.to_vec();
			let batch_len = batch.len();

			let semaphore = Arc::new(Semaphore::new(config.download_concurrency));
			let http_client = &client;

			let downloaded: Vec<(Post, DynamicImage)> = stream::iter(batch)
				.map(|post| {
					let sem = semaphore.clone();
					async move {
						let _permit = sem.acquire().await.unwrap();
						let url = post.preview_url.clone();
						match http_client.download_preview(&url).await {
							Ok(data) => {
								if let Some(img) = validate_downloaded_image(post.id, &data) {
									Some((post, img))
								} else {
									None
								}
							}
							Err(e) => {
								tracing::warn!(post_id = post.id, error = %e, "download failed");
								None
							}
						}
					}
				})
				.buffer_unordered(config.download_concurrency)
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

			let embedder_clone = embedder.clone();
			let new_last = posts_for_embed
				.iter()
				.map(|p| p.id)
				.max()
				.unwrap_or(last_id);
			let embeddings =
				tokio::task::spawn_blocking(move || -> Result<Vec<PostEmbedding>, RoobuError> {
					let preprocessed: Vec<DynamicImage> =
						images.iter().map(|img| Embedder::preprocess(img)).collect();

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

			store.upsert(embeddings).await?;

			if new_last > last_id {
				last_id = new_last;
				checkpoint::set(&mut ckpt, site, last_id);
				checkpoint::save(checkpoint_path, &ckpt)?;
			}

			ui_success!(
				"{}",
				format!(
					"Upserted {} posts  ·  checkpoint {}",
					valid_count.bold().bright_white(),
					last_id.bold().bright_white()
				)
				.as_str()
			);
		}

		tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)).await;
	}
}

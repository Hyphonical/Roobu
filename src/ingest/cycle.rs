//! Ingest cycle logic — single-site fetch, download, embed, and upsert.
//!
//! Handles one complete cycle of fetching new posts from a site, downloading
//! thumbnails, embedding them, and upserting into Qdrant.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures::stream::{self, StreamExt};
use image::DynamicImage;
use owo_colors::OwoColorize;
use tokio::sync::{Semaphore, mpsc};

use crate::embed::Embedder;
use crate::error::RoobuError;
use crate::ingest::checkpoint;
use crate::ingest::checkpoint::CheckpointMap;
use crate::ingest::events::IngestEvent;
use crate::sites::{BooruClient, Post, validate_downloaded_image};
use crate::store::{PostEmbedding, Store};
use crate::{ui_detail, ui_step, ui_success, ui_warn};

/// Configuration for the ingest pipeline.
pub struct IngestConfig {
	pub poll_interval_secs: u64,
	pub batch_size: usize,
	pub download_concurrency: usize,
	pub site_fetch_timeout_secs: u64,
	pub event_sink: Option<crate::ingest::IngestEventSink>,
}

/// State for a single site in the multi-site ingest loop.
pub struct SiteLoopState {
	pub client: crate::sites::SiteClient,
	pub site: &'static str,
	pub last_id: u64,
	pub resume_announced: bool,
}

/// Context shared across pipeline stages within a cycle.
pub struct CycleContext<'a> {
	pub store: &'a Store,
	pub embedder: Arc<Embedder>,
	pub checkpoint_path: &'a Path,
	pub config: &'a IngestConfig,
	pub ckpt: &'a mut CheckpointMap,
}

/// Statistics for a single ingest cycle.
pub struct CycleStats {
	pub fetched_posts: usize,
	pub valid_images: usize,
	pub skipped_images: usize,
	pub upserted_posts: usize,
	pub batch_count: usize,
	pub elapsed: Duration,
}

impl CycleStats {
	/// Create empty stats with the given elapsed time.
	pub fn empty(elapsed: Duration) -> Self {
		Self {
			fetched_posts: 0,
			valid_images: 0,
			skipped_images: 0,
			upserted_posts: 0,
			batch_count: 0,
			elapsed,
		}
	}

	/// Posts upserted per second.
	pub fn posts_per_second(&self) -> f64 {
		let secs = self.elapsed.as_secs_f64();
		if self.upserted_posts == 0 || secs <= f64::EPSILON {
			0.0
		} else {
			self.upserted_posts as f64 / secs
		}
	}

	/// Seconds per post upserted.
	pub fn seconds_per_post(&self) -> f64 {
		let secs = self.elapsed.as_secs_f64();
		if self.upserted_posts == 0 || secs <= f64::EPSILON {
			0.0
		} else {
			secs / self.upserted_posts as f64
		}
	}
}

/// Format a duration as a human-readable string.
pub fn format_elapsed(duration: Duration) -> String {
	format!("{:.2}s", duration.as_secs_f64())
}

/// Print cycle statistics to the terminal.
pub fn print_cycle_stats(stats: &CycleStats) {
	ui_detail!(
		"Cycle",
		"{} total  ·  {} upserted  ·  {} posts/s  ·  {} s/post",
		format_elapsed(stats.elapsed).bold().bright_white(),
		stats.upserted_posts.bold().bright_white(),
		format!("{:.2}", stats.posts_per_second())
			.bold()
			.bright_white(),
		format!("{:.3}", stats.seconds_per_post())
			.bold()
			.bright_white()
	);

	ui_detail!(
		"Totals",
		"{} fetched  ·  {} valid  ·  {} skipped  ·  {} batches",
		stats.fetched_posts.bold().bright_white(),
		stats.valid_images.bold().bright_white(),
		stats.skipped_images.bold().bright_white(),
		stats.batch_count.bold().bright_white()
	);
}

/// Download a batch of post thumbnails concurrently.
pub async fn download_batch(
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
				let permit = match sem.acquire().await {
					Ok(p) => p,
					Err(e) => {
						tracing::warn!(post_id = post.id, error = %e, "semaphore closed; skipping download");
						return None;
					}
				};
				let _permit = permit;
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

/// A batch of downloaded posts with their images.
pub struct DownloadedBatch {
	pub batch_len: usize,
	pub downloaded: Vec<(Post, DynamicImage)>,
}

/// Embed a downloaded batch and return the results.
pub async fn embed_downloaded_batch(
	embedder: &Arc<Embedder>,
	batch: DownloadedBatch,
) -> anyhow::Result<Option<EmbeddedBatch>> {
	let DownloadedBatch {
		batch_len,
		downloaded,
	} = batch;

	if downloaded.is_empty() {
		ui_warn!("batch had no valid images after download");
		return Ok(None);
	}

	let valid_count = downloaded.len();
	let skipped = batch_len - valid_count;

	ui_detail!(
		"Valid",
		"{} images{}",
		valid_count.bold().bright_white(),
		if skipped > 0 {
			format!("  ·  {} skipped", skipped)
		} else {
			String::new()
		}
	);

	let posts_for_embed: Vec<Post> = downloaded.iter().map(|(p, _)| p.clone()).collect();
	let images: Vec<DynamicImage> = downloaded.into_iter().map(|(_, img)| img).collect();

	let embedder_clone = embedder.clone();
	let new_last = posts_for_embed.iter().map(|p| p.id).max().unwrap_or(0);
	let ingestion_date = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_secs() as i64)
		.unwrap_or_default();
	let embeddings =
		tokio::task::spawn_blocking(move || -> Result<Vec<PostEmbedding>, RoobuError> {
			let preprocessed: Vec<DynamicImage> = images.iter().map(Embedder::preprocess).collect();

			let image_vecs = embedder_clone.embed_images(&preprocessed)?;

			let mut results = Vec::with_capacity(posts_for_embed.len());
			for (post, image_vec) in posts_for_embed.into_iter().zip(image_vecs.into_iter()) {
				results.push(PostEmbedding {
					post_id: post.id,
					site: post.site,
					site_namespace: post.site_namespace,
					post_url: post.post_url(),
					thumbnail_url: post.thumbnail_url.clone(),
					direct_image_url: post.preferred_image_url(),
					tags: post.tags,
					width: post.width,
					height: post.height,
					ingestion_date,
					rating: post.rating.clone(),
					image_vec,
				});
			}
			Ok(results)
		})
		.await??;

	Ok(Some(EmbeddedBatch {
		valid_count,
		skipped_count: skipped,
		new_last,
		embeddings,
	}))
}

/// A batch of embedded posts ready for upsertion.
pub struct EmbeddedBatch {
	pub valid_count: usize,
	pub skipped_count: usize,
	pub new_last: u64,
	pub embeddings: Vec<PostEmbedding>,
}

/// Process an embedded batch by upserting into Qdrant and updating the checkpoint.
async fn process_embedded_batch(
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
	batch: EmbeddedBatch,
) -> anyhow::Result<()> {
	let EmbeddedBatch {
		valid_count,
		new_last,
		embeddings,
		..
	} = batch;

	context.store.upsert(embeddings).await?;

	if new_last > *last_id {
		*last_id = new_last;
		checkpoint::set(context.ckpt, site, *last_id);
		checkpoint::save(context.checkpoint_path, context.ckpt)?;
		if let Some(sink) = &context.config.event_sink {
			sink(IngestEvent::CheckpointUpdated {
				site: site.to_string(),
				last_id: *last_id,
			});
		}
	}

	ui_success!(
		"Upserted {} posts  ·  checkpoint {}",
		valid_count.bold().bright_white(),
		(*last_id).bold().bright_white()
	);

	Ok(())
}

/// Run a single ingest cycle for a site.
///
/// Fetches new posts, downloads thumbnails, embeds them, and upserts into Qdrant.
pub async fn run_cycle(
	client: &impl BooruClient,
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
) -> anyhow::Result<CycleStats> {
	use std::time::Instant;

	let cycle_start = Instant::now();
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
		ui_step!("No new posts");
		tracing::debug!(site, "no new posts");
		return Ok(CycleStats::empty(cycle_start.elapsed()));
	}

	ui_step!("Fetched {} new posts", posts.len().bold().bright_white());

	let batch_size = context.config.batch_size;
	let download_concurrency = context.config.download_concurrency;
	if batch_size == 0 {
		anyhow::bail!("{site}: batch size must be greater than 0")
	}
	if download_concurrency == 0 {
		anyhow::bail!("{site}: download concurrency must be greater than 0")
	}

	const DOWNLOAD_QUEUE_MAX_DEPTH: usize = 4;
	const EMBEDDING_QUEUE_MAX_DEPTH: usize = 4;

	let fetched_posts = posts.len();
	let batch_count = fetched_posts.div_ceil(batch_size);

	let download_queue_depth =
		(fetched_posts.div_ceil(batch_size)).clamp(1, DOWNLOAD_QUEUE_MAX_DEPTH);
	let embedding_queue_depth =
		(fetched_posts.div_ceil(batch_size)).clamp(1, EMBEDDING_QUEUE_MAX_DEPTH);
	let (download_tx, mut download_rx) = mpsc::channel::<DownloadedBatch>(download_queue_depth);
	let (embedding_tx, mut embedding_rx) = mpsc::channel::<EmbeddedBatch>(embedding_queue_depth);

	// Move sender ownership into the producer so channel closure is observable by the consumer.
	let producer = async move {
		for batch in posts.chunks(batch_size).map(|chunk| chunk.to_vec()) {
			let downloaded = download_batch(client, batch, download_concurrency).await;
			if download_tx.send(downloaded).await.is_err() {
				break;
			}
		}
		Ok::<(), anyhow::Error>(())
	};

	let embedder = context.embedder.clone();
	let embed_stage = async move {
		while let Some(downloaded) = download_rx.recv().await {
			if let Some(embedded) = embed_downloaded_batch(&embedder, downloaded).await?
				&& embedding_tx.send(embedded).await.is_err()
			{
				break;
			}
		}
		Ok::<(), anyhow::Error>(())
	};

	let upsert_stage = async {
		let mut valid_images = 0usize;
		let mut skipped_images = 0usize;
		let mut upserted_posts = 0usize;

		while let Some(embedded) = embedding_rx.recv().await {
			let valid_count = embedded.valid_count;
			let skipped_count = embedded.skipped_count;
			process_embedded_batch(site, last_id, context, embedded).await?;
			valid_images += valid_count;
			skipped_images += skipped_count;
			upserted_posts += valid_count;
		}

		Ok::<(usize, usize, usize), anyhow::Error>((valid_images, skipped_images, upserted_posts))
	};

	let (_, _, (valid_images, skipped_images, upserted_posts)) =
		tokio::try_join!(producer, embed_stage, upsert_stage)?;

	Ok(CycleStats {
		fetched_posts,
		valid_images,
		skipped_images,
		upserted_posts,
		batch_count,
		elapsed: cycle_start.elapsed(),
	})
}

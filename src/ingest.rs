//! Ingest command adapters — bridge between CLI args and the ingest pipeline.

use std::path::PathBuf;
use std::sync::Arc;

use crate::embed::Embedder;
use crate::ingest::{self, IngestConfig};
use crate::sites::{BooruClient, SiteClient};
use crate::store::Store;

/// Run the ingest loop for a single site.
pub async fn run(
	client: impl BooruClient,
	store: &Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &std::path::Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	ingest::run(client, store, embedder, checkpoint_path, config).await
}

/// Run the ingest loop for multiple sites sequentially.
pub async fn run_multi(
	clients: Vec<SiteClient>,
	store: &Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &std::path::Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	ingest::run_multi(clients, store, embedder, checkpoint_path, config).await
}
	clients: Vec<SiteClient>,
	store: &Store,
	embedder: Arc<Embedder>,
	checkpoint_path: &Path,
	config: &IngestConfig,
) -> anyhow::Result<()> {
	ingest::run_multi(clients, store, embedder, checkpoint_path, config).await
}

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

struct EmbeddedBatch {
	valid_count: usize,
	skipped_count: usize,
	new_last: u64,
	embeddings: Vec<PostEmbedding>,
}

struct CycleStats {
	fetched_posts: usize,
	valid_images: usize,
	skipped_images: usize,
	upserted_posts: usize,
	batch_count: usize,
	elapsed: Duration,
}

impl CycleStats {
	fn empty(elapsed: Duration) -> Self {
		Self {
			fetched_posts: 0,
			valid_images: 0,
			skipped_images: 0,
			upserted_posts: 0,
			batch_count: 0,
			elapsed,
		}
	}

	fn posts_per_second(&self) -> f64 {
		let secs = self.elapsed.as_secs_f64();
		if self.upserted_posts == 0 || secs <= f64::EPSILON {
			0.0
		} else {
			self.upserted_posts as f64 / secs
		}
	}

	fn seconds_per_post(&self) -> f64 {
		let secs = self.elapsed.as_secs_f64();
		if self.upserted_posts == 0 || secs <= f64::EPSILON {
			0.0
		} else {
			secs / self.upserted_posts as f64
		}
	}
}

const DOWNLOAD_QUEUE_MAX_DEPTH: usize = 4;
const EMBEDDING_QUEUE_MAX_DEPTH: usize = 4;

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

async fn embed_downloaded_batch(
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
	let new_last = posts_for_embed.iter().map(|p| p.id).max().unwrap_or(0);
	let ingestion_date = SystemTime::now()
		.duration_since(UNIX_EPOCH)
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

fn format_elapsed(duration: Duration) -> String {
	format!("{:.2}s", duration.as_secs_f64())
}

fn print_cycle_stats(stats: &CycleStats) {
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

async fn run_cycle(
	client: &impl BooruClient,
	site: &'static str,
	last_id: &mut u64,
	context: &mut CycleContext<'_>,
) -> anyhow::Result<CycleStats> {
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
		ui_step!("{}", "No new posts");
		tracing::debug!(site, "no new posts");
		return Ok(CycleStats::empty(cycle_start.elapsed()));
	}

	ui_step!(
		"{}",
		format!("Fetched {} new posts", posts.len().bold().bright_white()).as_str()
	);

	let batch_size = context.config.batch_size;
	let download_concurrency = context.config.download_concurrency;
	if batch_size == 0 {
		anyhow::bail!("{site}: batch size must be greater than 0")
	}
	if download_concurrency == 0 {
		anyhow::bail!("{site}: download concurrency must be greater than 0")
	}

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
		let cycle_start = Instant::now();
		match run_cycle(&client, site, &mut last_id, &mut context).await {
			Ok(stats) => print_cycle_stats(&stats),
			Err(error) => {
				let elapsed = cycle_start.elapsed();
				let error_chain = format!("{error:#}");
				ui_warn!(
					"{}",
					format!(
						"{site} cycle failed ({error_chain}) after {} · skipping until next poll",
						format_elapsed(elapsed)
					)
					.as_str()
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
			match run_cycle(&state.client, state.site, &mut state.last_id, &mut context).await {
				Ok(cycle_stats) => print_cycle_stats(&cycle_stats),
				Err(error) => {
					let elapsed = cycle_start.elapsed();
					let error_chain = format!("{error:#}");
					ui_warn!(
						"{}",
						format!(
							"{} cycle failed ({error_chain}) after {} · skipping this site for now",
							state.site,
							format_elapsed(elapsed)
						)
						.as_str()
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

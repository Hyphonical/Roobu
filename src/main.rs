mod checkpoint;
mod embed;
mod error;
mod ingest;
mod sites;
mod store;
#[macro_use]
mod ui;

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;

#[derive(Parser)]
#[command(
	name = "roobu",
	version,
	about = "Semantic image search for booru sites."
)]
struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	Ingest {
		#[arg(long, env = "QDRANT_URL", default_value = "http://localhost:6334")]
		qdrant_url: String,

		#[arg(long, default_value = "models")]
		models_dir: PathBuf,

		#[arg(long, default_value = "checkpoint.json")]
		checkpoint: PathBuf,

		#[arg(long, default_value_t = 60)]
		poll_interval: u64,

		#[arg(long, default_value_t = 16)]
		batch_size: usize,

		#[arg(long, default_value_t = 8)]
		download_concurrency: usize,

		#[arg(long, env = "RULE34_API_KEY")]
		api_key: String,

		#[arg(long, env = "RULE34_USER_ID")]
		user_id: String,
	},

	Search {
		query: String,

		#[arg(short, long, default_value_t = 10)]
		limit: u64,

		#[arg(long, env = "QDRANT_URL", default_value = "http://localhost:6334")]
		qdrant_url: String,

		#[arg(long, default_value = "models")]
		models_dir: PathBuf,

		#[arg(long, default_value_t = 1.0)]
		weight: f32,

		#[arg(long)]
		site: Option<String>,
	},
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| "roobu=info".parse().unwrap()),
		)
		.init();

	let cli = Cli::parse();

	match cli.command {
		Commands::Ingest {
			qdrant_url,
			models_dir,
			checkpoint,
			poll_interval,
			batch_size,
			download_concurrency,
			api_key,
			user_id,
		} => {
			ui::header("roobu · init");

			ui_step!("{}", "Loading embedder…");
			let embedder = Arc::new(embed::Embedder::new(&models_dir)?);
			ui_success!("Embedder ready");

			ui_step!("{}", "Connecting to Qdrant…");
			let store = store::Store::new(&qdrant_url).await?;
			ui_success!("Qdrant ready");

			let client = sites::rule34::Rule34Client::new(api_key, user_id)?;

			let config = ingest::IngestConfig {
				poll_interval_secs: poll_interval,
				batch_size,
				download_concurrency,
			};

			ingest::run(client, &store, embedder, &checkpoint, &config).await?;
		}

		Commands::Search {
			query,
			limit,
			qdrant_url,
			models_dir,
			weight,
			site,
		} => {
			ui::header("roobu · search");

			let embedder = embed::Embedder::new(&models_dir)?;
			let store = store::Store::new(&qdrant_url).await?;

			let image_weight = weight;
			let tags_weight = 1.0 - weight;

			ui_step!(
				"{}",
				format!(
					"\"{}\"  ·  image={:.1} tags={:.1}",
					query.bright_white().bold(),
					image_weight,
					tags_weight
				)
				.as_str()
			);

			let query_vec = tokio::task::spawn_blocking({
				let q = query.clone();
				move || embedder.embed_text(&q)
			})
			.await??;

			let results = store
				.search(
					query_vec.to_vec(),
					image_weight,
					tags_weight,
					limit,
					site.as_deref(),
				)
				.await?;

			println!();
			if results.is_empty() {
				ui_warn!("No results found");
			} else {
				for r in &results {
					println!(
						"  {}    {}  {}",
						format!("#{}", r.post_id).bright_white().bold(),
						format!("{:.4}", r.score).dimmed(),
						r.post_url.cyan(),
					);
				}
				println!();
				ui_success!(
					"{}",
					format!("{} results", results.len().bold().bright_white()).as_str()
				);
			}
		}
	}

	Ok(())
}

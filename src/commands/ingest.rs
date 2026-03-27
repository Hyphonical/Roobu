use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;

use crate::embed::{self, OnnxOptimizationIntensity};
use crate::ingest;
use crate::sites;
use crate::store;
use crate::ui::{header, ui_step, ui_success};

pub struct Args {
	pub site: Option<sites::SiteKind>,
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

fn site_credentials(args: &Args) -> sites::SiteCredentials {
	sites::SiteCredentials {
		rule34_api_key: args.rule34_api_key.clone(),
		rule34_user_id: args.rule34_user_id.clone(),
		e621_login: args.e621_login.clone(),
		e621_api_key: args.e621_api_key.clone(),
	}
}

fn build_all_sites_clients(args: &Args) -> anyhow::Result<Vec<sites::SiteClient>> {
	let mut clients = Vec::new();

	match (&args.rule34_api_key, &args.rule34_user_id) {
		(Some(_), Some(_)) => clients.push(sites::build_client(
			sites::SiteKind::Rule34,
			site_credentials(args),
		)?),
		(None, None) => ui_step!(
			"{}",
			"RULE34_API_KEY and RULE34_USER_ID not set; skipping rule34 in all-sites mode"
		),
		_ => {
			anyhow::bail!(
				"RULE34_API_KEY and RULE34_USER_ID must both be set to include rule34 in all-sites mode"
			)
		}
	}

	clients.push(sites::build_client(
		sites::SiteKind::E621,
		site_credentials(args),
	)?);

	if clients.is_empty() {
		anyhow::bail!("no ingest clients available")
	}

	Ok(clients)
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

	match args.site {
		Some(site) => {
			let client = sites::build_client(site, site_credentials(&args))
				.with_context(|| format!("failed to build {site:?} client"))?;
			ingest::run(client, &store, embedder, &args.checkpoint, &ingest_config).await
		}
		None => {
			let clients = build_all_sites_clients(&args)?;
			ui_success!(
				"{}",
				format!(
					"All-sites mode enabled · {} clients (sequential)",
					clients.len()
				)
				.as_str()
			);
			ingest::run_multi(clients, &store, embedder, &args.checkpoint, &ingest_config).await
		}
	}
}

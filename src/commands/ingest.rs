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
	pub kemono_session: Option<String>,
	pub kemono_base_url: Option<String>,
	pub onnx_optimization: OnnxOptimizationIntensity,
}

fn site_credentials(args: &Args) -> sites::SiteCredentials {
	sites::SiteCredentials {
		rule34_api_key: args.rule34_api_key.clone(),
		rule34_user_id: args.rule34_user_id.clone(),
		e621_login: args.e621_login.clone(),
		e621_api_key: args.e621_api_key.clone(),
		kemono_session: args.kemono_session.clone(),
		kemono_base_url: args.kemono_base_url.clone(),
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
	clients.push(sites::build_client(
		sites::SiteKind::Safebooru,
		site_credentials(args),
	)?);
	clients.push(sites::build_client(
		sites::SiteKind::Xbooru,
		site_credentials(args),
	)?);
	clients.push(sites::build_client(
		sites::SiteKind::Kemono,
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

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use crate::embed::OnnxOptimizationIntensity;
	use crate::sites::BooruClient;

	use super::{Args, build_all_sites_clients};

	fn default_args() -> Args {
		Args {
			site: None,
			qdrant_url: "http://localhost:6334".to_string(),
			models_dir: PathBuf::from("models"),
			checkpoint: PathBuf::from("checkpoint.json"),
			poll_interval: 60,
			batch_size: 16,
			download_concurrency: 8,
			rule34_api_key: None,
			rule34_user_id: None,
			e621_login: None,
			e621_api_key: None,
			kemono_session: None,
			kemono_base_url: None,
			onnx_optimization: OnnxOptimizationIntensity::Safe,
		}
	}

	#[test]
	fn all_sites_mode_includes_e621_and_safebooru_without_rule34_credentials() {
		let args = default_args();
		let clients = build_all_sites_clients(&args).expect("all-sites clients should build");

		let site_names: Vec<&str> = clients.iter().map(|client| client.site_name()).collect();
		assert_eq!(site_names, vec!["e621", "safebooru", "xbooru", "kemono"]);
	}

	#[test]
	fn all_sites_mode_includes_rule34_when_credentials_are_present() {
		let mut args = default_args();
		args.rule34_api_key = Some("api-key".to_string());
		args.rule34_user_id = Some("user-id".to_string());

		let clients = build_all_sites_clients(&args).expect("all-sites clients should build");
		let site_names: Vec<&str> = clients.iter().map(|client| client.site_name()).collect();

		assert_eq!(
			site_names,
			vec!["rule34", "e621", "safebooru", "xbooru", "kemono"]
		);
	}
}

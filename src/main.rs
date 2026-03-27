mod checkpoint;
mod cli;
mod commands;
mod config;
mod embed;
mod error;
mod ingest;
mod sites;
mod store;
#[macro_use]
mod ui;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| config::DEFAULT_TRACING_FILTER.parse().unwrap()),
		)
		.init();

	let cli = cli::Cli::parse();
	commands::run(cli.command).await
}

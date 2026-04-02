//! Roobu — Semantic image search for booru sites.
//!
//! This application indexes images from multiple booru-style image boards,
//! embeds them using ONNX-based SigLIP models, and stores the resulting
//! vectors in Qdrant for semantic search.

mod checkpoint;
mod cli;
mod commands;
mod config;
mod embed;
mod error;
mod ingest;
mod sites;
mod store;
mod ui;
mod web;

use clap::Parser;

/// Application entry point. Handles error formatting and exit codes.
#[tokio::main]
async fn main() {
	if let Err(error) = run().await {
		eprintln!("Error: {error}");
		for cause in error.chain().skip(1) {
			eprintln!("  caused by: {cause}");
		}
		std::process::exit(1);
	}
}

/// Initialize tracing, parse CLI arguments, and dispatch to the appropriate command handler.
async fn run() -> anyhow::Result<()> {
	// Configure the tracing filter from environment or fall back to the project default.
	let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
		.or_else(|_| {
			config::DEFAULT_TRACING_FILTER.parse().map_err(|e| {
				eprintln!(
					"Warning: failed to parse default tracing filter '{}': {e}",
					config::DEFAULT_TRACING_FILTER
				);
				e
			})
		})
		.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("roobu=info"));

	tracing_subscriber::fmt().with_env_filter(env_filter).init();

	let cli = cli::Cli::parse();
	commands::run(cli.command).await
}

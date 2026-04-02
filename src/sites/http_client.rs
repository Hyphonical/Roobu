//! Shared HTTP client utilities for site adapters.
//!
//! Provides a standardized [`reqwest::Client`] configuration and retry logic
//! used across all booru site adapters.

use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

use crate::error::RoobuError;

// ── Retry Configuration ─────────────────────────────────────────────────────

/// Maximum number of retry attempts for transient failures.
pub const MAX_RETRIES: u32 = 6;
/// Initial backoff delay before the first retry.
pub const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
/// Maximum backoff delay (cap for exponential growth).
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Per-request timeout for individual HTTP requests.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ── Client Builder ──────────────────────────────────────────────────────────

/// Build a shared HTTP client with standard configuration for site adapters.
///
/// Sets a descriptive User-Agent header and a request timeout.
pub fn build_http_client() -> Result<Client, RoobuError> {
	let cargo_version = env!("CARGO_PKG_VERSION");
	Ok(Client::builder()
		.user_agent(format!("roobu/{cargo_version} (semantic search indexer)"))
		.timeout(REQUEST_TIMEOUT)
		.build()?)
}

// ── Request Helpers ─────────────────────────────────────────────────────────

/// Execute an HTTP GET request returning text with exponential backoff retry.
///
/// Retries on server errors (5xx) and rate limiting (429). Other status codes
/// are returned as errors immediately.
pub async fn get_text_with_retry(client: &Client, url: &str) -> Result<String, RoobuError> {
	let mut delay = INITIAL_BACKOFF;

	for attempt in 0..=MAX_RETRIES {
		let resp = client.get(url).send().await?;
		let status = resp.status();

		if status.is_success() {
			return Ok(resp.text().await?);
		}

		if (status.is_server_error() || status.as_u16() == 429) && attempt < MAX_RETRIES {
			tracing::warn!(status = %status, attempt, url, "retrying after backoff");
			sleep(delay).await;
			delay = (delay * 2).min(MAX_BACKOFF);
			continue;
		}

		return Err(RoobuError::Api(format!("HTTP {status}")));
	}

	Err(RoobuError::Api("max retries exceeded".into()))
}

/// Download raw bytes from a URL with error-for-status check.
pub async fn download_bytes(client: &Client, url: &str) -> Result<bytes::Bytes, RoobuError> {
	let resp = client.get(url).send().await?.error_for_status()?;
	Ok(resp.bytes().await?)
}

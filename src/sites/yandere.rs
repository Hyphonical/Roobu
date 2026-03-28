use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://yande.re/post.json";
const SITE_NAME: &str = "yandere";
const SITE_NAMESPACE: u64 = 11;
const MAX_POSTS_PER_PAGE: u16 = 100;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub struct YandereClient {
	http: Client,
}

impl YandereClient {
	pub fn new() -> Result<Self, RoobuError> {
		let cargo_version = env!("CARGO_PKG_VERSION");
		let http = Client::builder()
			.user_agent(format!("roobu/{} (semantic search indexer)", cargo_version))
			.timeout(Duration::from_secs(30))
			.build()?;

		Ok(Self { http })
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&page=1&tags=order:id_desc");

		let mut delay = INITIAL_BACKOFF;

		for attempt in 0..=MAX_RETRIES {
			let resp = self.http.get(&url).send().await?;
			let status = resp.status();

			if status.is_success() {
				let body = resp.text().await?;
				return Ok(body);
			}

			if (status.is_server_error() || status.as_u16() == 429) && attempt < MAX_RETRIES {
				tracing::warn!(status = %status, attempt, "retrying after backoff");
				sleep(delay).await;
				delay = (delay * 2).min(MAX_BACKOFF);
				continue;
			}

			return Err(RoobuError::Api(format!("HTTP {status}")));
		}

		Err(RoobuError::Api("max retries exceeded".into()))
	}
}

#[derive(Debug, Deserialize)]
struct RawPost {
	id: u64,
	#[serde(default)]
	tags: String,
	#[serde(default)]
	preview_url: Option<String>,
	#[serde(default)]
	sample_url: Option<String>,
	#[serde(default)]
	jpeg_url: Option<String>,
	#[serde(default)]
	file_url: Option<String>,
	#[serde(default)]
	width: u32,
	#[serde(default)]
	height: u32,
	#[serde(default)]
	rating: String,
}

impl RawPost {
	fn into_post(self) -> Post {
		Post {
			id: self.id,
			tags: self.tags,
			preview_url: self
				.preview_url
				.or(self.sample_url)
				.or(self.jpeg_url)
				.or(self.file_url)
				.unwrap_or_default(),
			width: self.width,
			height: self.height,
			rating: self.rating,
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: None,
		}
	}
}

impl BooruClient for YandereClient {
	fn site_name(&self) -> &'static str {
		SITE_NAME
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		let body = self.fetch_page_raw().await?;

		if body.is_empty() || body.starts_with('<') {
			tracing::debug!("empty or HTML response from API, returning empty");
			return Ok(Vec::new());
		}

		let raw: Vec<RawPost> = serde_json::from_str(&body)
			.map_err(|e| RoobuError::Api(format!("JSON parse error: {e}")))?;

		let posts: Vec<Post> = raw
			.into_iter()
			.map(RawPost::into_post)
			.filter(|p| p.id > last_id)
			.collect();

		Ok(posts)
	}

	async fn download_preview(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
		let resp = self.http.get(url).send().await?.error_for_status()?;
		let bytes = resp.bytes().await?;
		Ok(bytes)
	}
}

#[cfg(test)]
mod tests {
	use super::RawPost;

	#[test]
	fn into_post_falls_back_to_jpeg_url() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 404,
				"tags": "foo",
				"preview_url": null,
				"sample_url": null,
				"jpeg_url": "https://yande.re/jpeg.jpg",
				"file_url": "https://yande.re/file.png",
				"width": 1200,
				"height": 800,
				"rating": "q"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();
		assert_eq!(post.preview_url, "https://yande.re/jpeg.jpg");
	}
}

use reqwest::Client;
use serde::Deserialize;

use super::common::first_url_or_empty;
use super::http_client::{build_http_client, download_bytes, get_text_with_retry};
use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://konachan.com/post.json";
const SITE_NAME: &str = "konachan";
const SITE_NAMESPACE: u64 = 10;
const MAX_POSTS_PER_PAGE: u16 = 100;

pub struct KonachanClient {
	http: Client,
}

impl KonachanClient {
	pub fn new() -> Result<Self, RoobuError> {
		Ok(Self {
			http: build_http_client()?,
		})
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&page=1&tags=order:id_desc");
		get_text_with_retry(&self.http, &url).await
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
		let direct_image_url = self.file_url.clone().or(self.jpeg_url.clone());
		let thumbnail_url = first_url_or_empty([
			self.preview_url,
			self.sample_url,
			self.jpeg_url,
			self.file_url,
		]);

		Post {
			id: self.id,
			tags: self.tags,
			thumbnail_url,
			direct_image_url,
			width: self.width,
			height: self.height,
			rating: self.rating,
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: None,
		}
	}
}

impl BooruClient for KonachanClient {
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

	async fn download_thumbnail(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
		download_bytes(&self.http, url).await
	}
}

#[cfg(test)]
mod tests {
	use super::RawPost;

	#[test]
	fn into_post_prefers_preview_then_sample() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 999,
				"tags": "foo bar",
				"preview_url": null,
				"sample_url": "https://konachan.com/sample.jpg",
				"jpeg_url": "https://konachan.com/jpeg.jpg",
				"file_url": "https://konachan.com/file.png",
				"width": 1920,
				"height": 1080,
				"rating": "s"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();
		assert_eq!(post.thumbnail_url, "https://konachan.com/sample.jpg");
		assert_eq!(post.tags, "foo bar");
	}
}

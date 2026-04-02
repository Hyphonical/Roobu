use reqwest::Client;
use serde::Deserialize;

use super::common::first_url_or_empty;
use super::http_client::{build_http_client, download_bytes, get_text_with_retry};
use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://aibooru.online/posts.json";
const SITE_NAME: &str = "aibooru";
const SITE_NAMESPACE: u64 = 8;
const MAX_POSTS_PER_PAGE: u16 = 200;

pub struct AibooruClient {
	http: Client,
}

impl AibooruClient {
	pub fn new() -> Result<Self, RoobuError> {
		Ok(Self {
			http: build_http_client()?,
		})
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&tags=order:id_desc");
		get_text_with_retry(&self.http, &url).await
	}
}

#[derive(Debug, Deserialize)]
struct RawPost {
	id: u64,
	#[serde(default)]
	tag_string: String,
	#[serde(default)]
	preview_file_url: Option<String>,
	#[serde(default)]
	large_file_url: Option<String>,
	#[serde(default)]
	file_url: Option<String>,
	#[serde(default)]
	image_width: u32,
	#[serde(default)]
	image_height: u32,
	#[serde(default)]
	rating: String,
}

impl RawPost {
	fn into_post(self) -> Post {
		let direct_image_url = self.file_url.clone().or(self.large_file_url.clone());
		let thumbnail_url =
			first_url_or_empty([self.preview_file_url, self.large_file_url, self.file_url]);

		Post {
			id: self.id,
			tags: self.tag_string,
			thumbnail_url,
			direct_image_url,
			width: self.image_width,
			height: self.image_height,
			rating: self.rating,
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: None,
		}
	}
}

impl BooruClient for AibooruClient {
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
	fn into_post_prefers_preview_file_url() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 123,
				"tag_string": "test_tag",
				"preview_file_url": "https://cdn.test/preview.jpg",
				"large_file_url": "https://cdn.test/large.jpg",
				"file_url": "https://cdn.test/file.jpg",
				"image_width": 1920,
				"image_height": 1080,
				"rating": "s"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();
		assert_eq!(post.thumbnail_url, "https://cdn.test/preview.jpg");
		assert_eq!(post.tags, "test_tag");
		assert_eq!(post.width, 1920);
		assert_eq!(post.height, 1080);
	}

	#[test]
	fn into_post_falls_back_to_file_url() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 321,
				"tag_string": "",
				"preview_file_url": null,
				"large_file_url": null,
				"file_url": "https://cdn.test/file-only.jpg",
				"image_width": 100,
				"image_height": 200,
				"rating": "q"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();
		assert_eq!(post.thumbnail_url, "https://cdn.test/file-only.jpg");
	}
}

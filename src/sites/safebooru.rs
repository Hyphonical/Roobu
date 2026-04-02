use reqwest::Client;
use serde::Deserialize;

use super::common::first_url_or_empty;
use super::http_client::{build_http_client, download_bytes, get_text_with_retry};
use super::{BooruClient, Post};
use crate::error::RoobuError;

const BASE_URL: &str = "https://safebooru.org/index.php";
const SITE_NAME: &str = "safebooru";
const SITE_NAMESPACE: u64 = 3;

pub struct SafebooruClient {
	http: Client,
}

impl SafebooruClient {
	pub fn new() -> Result<Self, RoobuError> {
		Ok(Self {
			http: build_http_client()?,
		})
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{BASE_URL}?page=dapi&s=post&q=index&json=1&limit=100&pid=0");
		get_text_with_retry(&self.http, &url).await
	}
}

#[derive(Debug, Deserialize)]
struct RawPost {
	id: u64,
	#[serde(default)]
	tags: String,
	#[serde(default)]
	preview_url: String,
	#[serde(default)]
	file_url: Option<String>,
	#[serde(default)]
	sample_url: Option<String>,
	#[serde(default)]
	width: u32,
	#[serde(default)]
	height: u32,
	#[serde(default)]
	rating: String,
}

impl RawPost {
	fn into_post(self) -> Post {
		let direct_image_url = self.file_url.clone().or(self.sample_url.clone());
		let thumbnail_url =
			first_url_or_empty([Some(self.preview_url), self.sample_url, self.file_url]);

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

impl BooruClient for SafebooruClient {
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

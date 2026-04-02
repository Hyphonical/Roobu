use reqwest::Client;
use serde::Deserialize;

use super::common::first_url_or_empty;
use super::http_client::{build_http_client, download_bytes, get_text_with_retry};
use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://e6ai.net/posts.json";
const SITE_NAME: &str = "e6ai";
const SITE_NAMESPACE: u64 = 9;
const MAX_POSTS_PER_PAGE: u16 = 320;

pub struct E6AiClient {
	http: Client,
}

impl E6AiClient {
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
struct RawListing {
	#[serde(default)]
	posts: Vec<RawPost>,
}

#[derive(Debug, Deserialize)]
struct RawPost {
	id: u64,
	#[serde(default)]
	tags: RawTags,
	#[serde(default)]
	preview: RawPreview,
	#[serde(default)]
	sample: RawSample,
	#[serde(default)]
	file: RawFile,
	#[serde(default)]
	rating: String,
}

impl RawPost {
	fn into_post(self) -> Post {
		let RawFile {
			width,
			height,
			url: file_url,
		} = self.file;
		let direct_image_url = file_url.clone().or(self.sample.url.clone());
		let thumbnail_url = first_url_or_empty([self.preview.url, self.sample.url, file_url]);

		Post {
			id: self.id,
			tags: self.tags.into_tag_string(),
			thumbnail_url,
			direct_image_url,
			width,
			height,
			rating: self.rating,
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: None,
		}
	}
}

#[derive(Debug, Deserialize, Default)]
struct RawTags {
	#[serde(default)]
	general: Vec<String>,
	#[serde(default)]
	artist: Vec<String>,
	#[serde(default)]
	contributor: Vec<String>,
	#[serde(default)]
	copyright: Vec<String>,
	#[serde(default)]
	character: Vec<String>,
	#[serde(default)]
	species: Vec<String>,
	#[serde(default)]
	invalid: Vec<String>,
	#[serde(default)]
	meta: Vec<String>,
	#[serde(default)]
	lore: Vec<String>,
}

impl RawTags {
	fn into_tag_string(self) -> String {
		let mut tags = Vec::new();
		tags.extend(self.general);
		tags.extend(self.artist);
		tags.extend(self.contributor);
		tags.extend(self.copyright);
		tags.extend(self.character);
		tags.extend(self.species);
		tags.extend(self.invalid);
		tags.extend(self.meta);
		tags.extend(self.lore);
		tags.join(" ")
	}
}

#[derive(Debug, Deserialize, Default)]
struct RawPreview {
	#[serde(default)]
	url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSample {
	#[serde(default)]
	url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFile {
	#[serde(default)]
	width: u32,
	#[serde(default)]
	height: u32,
	#[serde(default)]
	url: Option<String>,
}

impl BooruClient for E6AiClient {
	fn site_name(&self) -> &'static str {
		SITE_NAME
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		let body = self.fetch_page_raw().await?;

		if body.is_empty() || body.starts_with('<') {
			tracing::debug!("empty or HTML response from API, returning empty");
			return Ok(Vec::new());
		}

		let raw: RawListing = serde_json::from_str(&body)
			.map_err(|e| RoobuError::Api(format!("JSON parse error: {e}")))?;

		let posts: Vec<Post> = raw
			.posts
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
	use super::{E6AiClient, RawPost};

	#[test]
	fn into_post_falls_back_to_sample_url() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 42,
				"file": {"width": 640, "height": 480, "url": "https://file.test/full.jpg"},
				"preview": {"url": null},
				"sample": {"url": "https://sample.test/sample.jpg"},
				"tags": {
					"general": ["blue_eyes"],
					"artist": ["someone"],
					"contributor": [],
					"copyright": [],
					"character": [],
					"species": [],
					"invalid": [],
					"meta": [],
					"lore": []
				},
				"rating": "s"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();

		assert_eq!(post.thumbnail_url, "https://sample.test/sample.jpg");
		assert_eq!(post.tags, "blue_eyes someone");
		assert_eq!(post.width, 640);
		assert_eq!(post.height, 480);
	}

	#[test]
	fn constructor_builds_without_credentials() {
		let result = E6AiClient::new();
		assert!(result.is_ok());
	}
}

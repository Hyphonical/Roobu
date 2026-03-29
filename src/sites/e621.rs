use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use super::common::first_url_or_empty;
use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://e621.net/posts.json";
const SITE_NAME: &str = "e621";
const SITE_NAMESPACE: u64 = 2;
const MAX_POSTS_PER_PAGE: u16 = 320;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub struct E621Client {
	http: Client,
	login: Option<String>,
	api_key: Option<String>,
}

impl E621Client {
	pub fn new(login: Option<String>, api_key: Option<String>) -> Result<Self, RoobuError> {
		if login.is_some() != api_key.is_some() {
			return Err(RoobuError::Api(
				"e621 credentials must include both login and api key".to_string(),
			));
		}

		let cargo_version = env!("CARGO_PKG_VERSION");
		let http = Client::builder()
			.user_agent(format!("roobu/{} (semantic search indexer)", cargo_version))
			.timeout(Duration::from_secs(30))
			.build()?;

		Ok(Self {
			http,
			login,
			api_key,
		})
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&tags=order:id_desc");

		let mut delay = INITIAL_BACKOFF;

		for attempt in 0..=MAX_RETRIES {
			let request = self.http.get(&url);
			let request = match (&self.login, &self.api_key) {
				(Some(login), Some(api_key)) => request.basic_auth(login, Some(api_key)),
				_ => request,
			};
			let resp = request.send().await?;
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

impl BooruClient for E621Client {
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
		let resp = self.http.get(url).send().await?.error_for_status()?;
		let bytes = resp.bytes().await?;
		Ok(bytes)
	}
}

#[cfg(test)]
mod tests {
	use super::{E621Client, RawPost};

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
	fn into_post_falls_back_to_file_url() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": 55,
				"file": {"width": 320, "height": 320, "url": "https://file.test/only.jpg"},
				"preview": {"url": null},
				"sample": {"url": null},
				"tags": {
					"general": [],
					"artist": [],
					"contributor": [],
					"copyright": [],
					"character": [],
					"species": [],
					"invalid": [],
					"meta": [],
					"lore": []
				},
				"rating": "e"
			}"#,
		)
		.expect("valid raw post json");

		let post = raw.into_post();

		assert_eq!(post.thumbnail_url, "https://file.test/only.jpg");
		assert_eq!(post.tags, "");
	}

	#[test]
	fn constructor_rejects_partial_credentials() {
		let result = E621Client::new(Some("user".to_string()), None);
		assert!(result.is_err());
		let message = result
			.err()
			.expect("constructor must reject partial credentials")
			.to_string();
		assert!(message.contains("both login and api key"));
	}
}

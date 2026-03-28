use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use super::{BooruClient, Post};
use crate::error::RoobuError;

const BASE_URL: &str = "https://gelbooru.com/index.php";
const SITE_NAME: &str = "gelbooru";
const SITE_NAMESPACE: u64 = 4;
const MAX_POSTS_PER_PAGE: u16 = 100;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub struct GelbooruClient {
	http: Client,
	api_key: String,
	user_id: String,
}

impl GelbooruClient {
	pub fn new(api_key: String, user_id: String) -> Result<Self, RoobuError> {
		let cargo_version = env!("CARGO_PKG_VERSION");
		let http = Client::builder()
			.user_agent(format!("roobu/{} (semantic search indexer)", cargo_version))
			.timeout(Duration::from_secs(30))
			.build()?;

		Ok(Self {
			http,
			api_key,
			user_id,
		})
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!(
			"{BASE_URL}?page=dapi&s=post&q=index&json=1&limit={MAX_POSTS_PER_PAGE}&pid=0&tags=sort:id:desc&api_key={}&user_id={}",
			self.api_key, self.user_id
		);

		let mut delay = INITIAL_BACKOFF;

		for attempt in 0..=MAX_RETRIES {
			let resp = self.http.get(&url).send().await?;
			let status = resp.status();

			if status.is_success() {
				let body = resp.text().await?;
				return Ok(body);
			}

			if status.as_u16() == 401 {
				return Err(RoobuError::Api(
					"Gelbooru API rejected credentials (HTTP 401). Check GELBOORU_API_KEY and GELBOORU_USER_ID".to_string(),
				));
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
#[serde(untagged)]
enum RawResponse {
	List(Vec<RawPost>),
	Wrapped {
		#[serde(default)]
		post: Vec<RawPost>,
	},
}

impl RawResponse {
	fn into_posts(self) -> Vec<RawPost> {
		match self {
			Self::List(posts) => posts,
			Self::Wrapped { post } => post,
		}
	}
}

#[derive(Debug, Deserialize)]
struct RawPost {
	#[serde(default)]
	id: serde_json::Value,
	#[serde(default)]
	tags: String,
	#[serde(default)]
	preview_url: String,
	#[serde(default)]
	width: serde_json::Value,
	#[serde(default)]
	height: serde_json::Value,
	#[serde(default)]
	rating: String,
}

impl RawPost {
	fn into_post(self) -> Option<Post> {
		Some(Post {
			id: parse_u64(&self.id)?,
			tags: self.tags,
			preview_url: self.preview_url,
			width: parse_u32(&self.width),
			height: parse_u32(&self.height),
			rating: self.rating,
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: None,
		})
	}
}

fn parse_u64(value: &serde_json::Value) -> Option<u64> {
	match value {
		serde_json::Value::String(v) => v.parse::<u64>().ok(),
		serde_json::Value::Number(v) => v.as_u64(),
		_ => None,
	}
}

fn parse_u32(value: &serde_json::Value) -> u32 {
	match value {
		serde_json::Value::String(v) => v.parse::<u32>().unwrap_or_default(),
		serde_json::Value::Number(v) => v.as_u64().unwrap_or_default() as u32,
		_ => 0,
	}
}

impl BooruClient for GelbooruClient {
	fn site_name(&self) -> &'static str {
		SITE_NAME
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		let body = self.fetch_page_raw().await?;

		if body.is_empty() || body.starts_with('<') {
			tracing::debug!("empty or HTML response from API, returning empty");
			return Ok(Vec::new());
		}

		let raw: RawResponse = serde_json::from_str(&body)
			.map_err(|e| RoobuError::Api(format!("JSON parse error: {e}")))?;

		let posts: Vec<Post> = raw
			.into_posts()
			.into_iter()
			.filter_map(RawPost::into_post)
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
	use super::{RawPost, RawResponse};

	#[test]
	fn parses_wrapped_gelbooru_response() {
		let raw: RawResponse = serde_json::from_str(
			r#"{
				"@attributes": {"count": 1},
				"post": [
					{
						"id": "123",
						"tags": "foo bar",
						"preview_url": "https://img.test/preview.jpg",
						"width": "1200",
						"height": "800",
						"rating": "general"
					}
				]
			}"#,
		)
		.expect("valid wrapped json");

		let posts = raw.into_posts();
		assert_eq!(posts.len(), 1);
	}

	#[test]
	fn into_post_handles_string_dimensions() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": "456",
				"tags": "alpha beta",
				"preview_url": "https://img.test/456.jpg",
				"width": "640",
				"height": "480",
				"rating": "safe"
			}"#,
		)
		.expect("valid post json");

		let post = raw.into_post().expect("post should convert");
		assert_eq!(post.id, 456);
		assert_eq!(post.width, 640);
		assert_eq!(post.height, 480);
		assert_eq!(post.preview_url, "https://img.test/456.jpg");
	}
}

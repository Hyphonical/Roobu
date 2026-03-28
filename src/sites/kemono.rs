use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::RwLock;
use std::time::Duration;
use tokio::time::sleep;

use super::{BooruClient, Post};
use crate::error::RoobuError;

const SITE_NAME: &str = "kemono";
const SITE_NAMESPACE: u64 = 7;
const POSTS_PAGE_SIZE: usize = 50;
const FETCH_PAGES_PER_CYCLE: usize = 6;
const MAX_RETRIES: u32 = 5;
const INITIAL_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const DEFAULT_BASE_URL: &str = "https://kemono.cr";
const KNOWN_BASE_URLS: [&str; 4] = [
	"https://kemono.cr",
];

pub struct KemonoClient {
	http: Client,
	session: Option<String>,
	preferred_base_url: RwLock<String>,
}

impl KemonoClient {
	pub fn new(session: Option<String>, base_url: Option<String>) -> Result<Self, RoobuError> {
		let cargo_version = env!("CARGO_PKG_VERSION");
		let http = Client::builder()
			.user_agent(format!("roobu/{} (semantic search indexer)", cargo_version))
			.timeout(Duration::from_secs(30))
			.default_headers(default_headers()?)
			.build()?;

		let normalized_session = normalize_optional(session);
		let preferred =
			normalize_optional(base_url).unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

		Ok(Self {
			http,
			session: normalized_session,
			preferred_base_url: RwLock::new(preferred),
		})
	}

	fn candidate_base_urls(&self) -> Vec<String> {
		let preferred = self
			.preferred_base_url
			.read()
			.map(|guard| guard.clone())
			.unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());

		let mut out = Vec::with_capacity(KNOWN_BASE_URLS.len() + 1);
		out.push(preferred.clone());
		for candidate in KNOWN_BASE_URLS {
			if candidate != preferred {
				out.push(candidate.to_string());
			}
		}
		out
	}

	fn set_preferred_base_url(&self, base_url: &str) {
		if let Ok(mut guard) = self.preferred_base_url.write() {
			*guard = base_url.to_string();
		}
	}

	fn with_optional_session_cookie(&self, req: RequestBuilder) -> RequestBuilder {
		match self.session.as_deref() {
			Some(token) => req.header(COOKIE, format!("session={token}")),
			None => req,
		}
	}

	async fn fetch_posts_page(
		&self,
		base_url: &str,
		offset: usize,
	) -> Result<RawListing, RoobuError> {
		let url = format!("{}/api/v1/posts?o={offset}", trim_trailing_slash(base_url));
		let mut delay = INITIAL_BACKOFF;

		for attempt in 0..=MAX_RETRIES {
			let request = self.with_optional_session_cookie(self.http.get(&url));
			let resp = request.send().await?;
			let status = resp.status();

			if status.is_success() {
				let body = resp.text().await?;
				let listing: RawListing = serde_json::from_str(&body)
					.map_err(|e| RoobuError::Api(format!("Kemono JSON parse error: {e}")))?;
				return Ok(listing);
			}

			if (status.is_server_error() || status.as_u16() == 429) && attempt < MAX_RETRIES {
				tracing::warn!(status = %status, attempt, base_url, offset, "kemono retrying after backoff");
				sleep(delay).await;
				delay = (delay * 2).min(MAX_BACKOFF);
				continue;
			}

			return Err(RoobuError::Api(format!("Kemono HTTP {status} from {url}")));
		}

		Err(RoobuError::Api("kemono max retries exceeded".into()))
	}

	async fn fetch_recent_from_base(
		&self,
		base_url: &str,
		last_id: u64,
	) -> Result<Vec<Post>, RoobuError> {
		let mut raw_posts = Vec::new();

		for page in 0..FETCH_PAGES_PER_CYCLE {
			let offset = page * POSTS_PAGE_SIZE;
			let listing = self.fetch_posts_page(base_url, offset).await?;
			if listing.posts.is_empty() {
				break;
			}
			raw_posts.extend(listing.posts);
		}

		if raw_posts.is_empty() {
			return Ok(Vec::new());
		}

		let mut seen = HashSet::new();
		let mut posts = Vec::new();

		for raw in raw_posts {
			let Some(post) = raw.into_post(base_url) else {
				continue;
			};
			if post.id <= last_id {
				continue;
			}
			if seen.insert(post.id) {
				posts.push(post);
			}
		}

		posts.sort_by_key(|p| p.id);
		Ok(posts)
	}
}

#[derive(Debug, Default, Deserialize)]
struct RawListing {
	#[serde(default)]
	posts: Vec<RawPost>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPost {
	#[serde(default)]
	id: serde_json::Value,
	#[serde(default)]
	title: String,
	#[serde(default)]
	substring: String,
	#[serde(default)]
	service: String,
	#[serde(default)]
	file: RawFile,
	#[serde(default)]
	attachments: Vec<RawAttachment>,
}

impl RawPost {
	fn into_post(self, base_url: &str) -> Option<Post> {
		let id = parse_id(&self.id)?;
		let media_path = self
			.file
			.path
			.as_deref()
			.filter(|path| is_supported_image_path(path))
			.map(ToOwned::to_owned)
			.or_else(|| {
				self.file
					.thumbnail
					.as_ref()
					.and_then(|thumb| thumb.path.as_deref())
					.filter(|path| is_supported_image_path(path))
					.map(ToOwned::to_owned)
			})
			.or_else(|| {
				self.attachments.iter().find_map(|attachment| {
					let path = attachment.path.as_deref()?;
					is_supported_image_path(path).then(|| path.to_string())
				})
			});

		let preview_url = media_path
			.as_deref()
			.map(|path| build_media_url(base_url, path))
			.unwrap_or_default();

		let tags = synthesize_tags(&self.title, &self.substring, &self.service);

		Some(Post {
			id,
			tags,
			preview_url,
			width: 0,
			height: 0,
			rating: String::new(),
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
		})
	}
}

#[derive(Debug, Default, Deserialize)]
struct RawFile {
	#[serde(default)]
	path: Option<String>,
	#[serde(default)]
	thumbnail: Option<RawThumbnail>,
}

#[derive(Debug, Default, Deserialize)]
struct RawThumbnail {
	#[serde(default)]
	path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAttachment {
	#[serde(default)]
	path: Option<String>,
}

impl BooruClient for KemonoClient {
	fn site_name(&self) -> &'static str {
		SITE_NAME
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		let mut last_error: Option<RoobuError> = None;

		for base_url in self.candidate_base_urls() {
			match self.fetch_recent_from_base(&base_url, last_id).await {
				Ok(posts) => {
					self.set_preferred_base_url(&base_url);
					if posts.is_empty() {
						tracing::debug!(base_url, "kemono returned no new posts");
					}
					return Ok(posts);
				}
				Err(err) => {
					tracing::warn!(base_url, error = %err, "kemono base failed, trying fallback");
					last_error = Some(err);
				}
			}
		}

		Err(last_error.unwrap_or_else(|| {
			RoobuError::Api("kemono fetch failed for all known domains".to_string())
		}))
	}

	async fn download_preview(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
		let req = self.with_optional_session_cookie(self.http.get(url));
		let resp = req.send().await?.error_for_status()?;
		let bytes = resp.bytes().await?;
		Ok(bytes)
	}
}

fn default_headers() -> Result<HeaderMap, RoobuError> {
	let mut headers = HeaderMap::new();
	headers.insert(
		reqwest::header::ACCEPT,
		HeaderValue::from_static("application/json, text/plain, */*"),
	);
	Ok(headers)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
	value.and_then(|v| {
		let trimmed = v.trim();
		if trimmed.is_empty() {
			None
		} else {
			Some(trimmed.to_string())
		}
	})
}

fn trim_trailing_slash(input: &str) -> &str {
	input.trim_end_matches('/')
}

fn parse_id(value: &serde_json::Value) -> Option<u64> {
	match value {
		serde_json::Value::String(s) => s.parse::<u64>().ok(),
		serde_json::Value::Number(n) => n.as_u64(),
		_ => None,
	}
}

fn strip_html_tags(input: &str) -> String {
	let mut out = String::with_capacity(input.len());
	let mut inside_tag = false;

	for ch in input.chars() {
		match ch {
			'<' => inside_tag = true,
			'>' => inside_tag = false,
			_ if !inside_tag => out.push(ch),
			_ => {}
		}
	}

	out
}

fn synthesize_tags(title: &str, substring: &str, service: &str) -> String {
	let title = title.trim();
	let summary = strip_html_tags(substring).replace('\n', " ");
	let summary = summary.trim();
	let service = service.trim();

	let mut parts = Vec::new();
	if !title.is_empty() {
		parts.push(title.to_string());
	}
	if !summary.is_empty() {
		parts.push(summary.to_string());
	}
	if !service.is_empty() {
		parts.push(service.to_string());
	}

	parts.join(" ")
}

fn is_supported_image_path(path: &str) -> bool {
	let lower = path.to_ascii_lowercase();
	lower.ends_with(".jpg")
		|| lower.ends_with(".jpeg")
		|| lower.ends_with(".png")
		|| lower.ends_with(".webp")
		|| lower.ends_with(".gif")
		|| lower.ends_with(".bmp")
}

fn build_media_url(base_url: &str, path: &str) -> String {
	if path.starts_with("http://") || path.starts_with("https://") {
		return path.to_string();
	}

	let clean_base = trim_trailing_slash(base_url);
	let clean_path = path.trim();
	if clean_path.starts_with("data/") {
		return format!("{clean_base}/{clean_path}");
	}
	let clean_path = if clean_path.starts_with('/') {
		clean_path
	} else {
		return format!("{clean_base}/data/{clean_path}");
	};

	if clean_path.starts_with("/data/") {
		format!("{clean_base}{clean_path}")
	} else {
		format!("{clean_base}/data{clean_path}")
	}
}

#[cfg(test)]
mod tests {
	use super::{RawPost, build_media_url, parse_id, strip_html_tags, synthesize_tags};

	#[test]
	fn parse_id_handles_string_and_number() {
		assert_eq!(parse_id(&serde_json::json!("123")), Some(123));
		assert_eq!(parse_id(&serde_json::json!(456)), Some(456));
		assert_eq!(parse_id(&serde_json::json!("abc")), None);
	}

	#[test]
	fn media_url_uses_data_prefix() {
		assert_eq!(
			build_media_url("https://kemono.cr", "/ab/cd/file.jpg"),
			"https://kemono.cr/data/ab/cd/file.jpg"
		);
		assert_eq!(
			build_media_url("https://kemono.cr/", "/data/ab/cd/file.jpg"),
			"https://kemono.cr/data/ab/cd/file.jpg"
		);
		assert_eq!(
			build_media_url("https://kemono.cr", "data/ab/cd/file.jpg"),
			"https://kemono.cr/data/ab/cd/file.jpg"
		);
	}

	#[test]
	fn strip_html_and_synthesize_tags() {
		assert_eq!(strip_html_tags("<p>Hello</p> world"), "Hello world");
		assert_eq!(
			synthesize_tags("Title", "<p>Description</p>", "patreon"),
			"Title Description patreon"
		);
	}

	#[test]
	fn picks_first_image_attachment_when_non_images_come_first() {
		let raw: RawPost = serde_json::from_str(
			r#"{
				"id": "123",
				"title": "Example",
				"substring": "",
				"service": "patreon",
				"file": {},
				"attachments": [
					{"path": "/00/00/archive.zip"},
					{"path": "/11/11/image.png"}
				]
			}"#,
		)
		.expect("valid raw post");

		let post = raw
			.into_post("https://kemono.cr")
			.expect("post should be convertible");
		assert_eq!(post.preview_url, "https://kemono.cr/data/11/11/image.png");
	}
}

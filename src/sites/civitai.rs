use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://civitai.com/api/v1/images";
const SITE_NAME: &str = "civitai";
const SITE_NAMESPACE: u64 = 12;
const MEDIA_CACHE_BASE_URL: &str = "https://image-b2.civitai.com/file/civitai-media-cache";
const MEDIA_CACHE_SIZE_SEGMENT: &str = "450x%3Cauto%3E_so";
const MAX_POSTS_PER_PAGE: u16 = 200;
const MAX_RETRIES: u32 = 6;
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub struct CivitaiClient {
	http: Client,
}

impl CivitaiClient {
	pub fn new() -> Result<Self, RoobuError> {
		let cargo_version = env!("CARGO_PKG_VERSION");
		let http = Client::builder()
			.user_agent(format!("roobu/{} (semantic search indexer)", cargo_version))
			.timeout(Duration::from_secs(30))
			.build()?;

		Ok(Self { http })
	}

	async fn fetch_page_raw(&self) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&sort=Newest");

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

#[derive(Debug, Deserialize, Default)]
struct RawListing {
	#[serde(default)]
	items: Vec<RawImage>,
}

#[derive(Debug, Deserialize, Default)]
struct RawImage {
	id: u64,
	#[serde(default)]
	url: Option<String>,
	#[serde(default)]
	width: u32,
	#[serde(default)]
	height: u32,
	#[serde(default, rename = "nsfwLevel")]
	nsfw_level: String,
	#[serde(default)]
	nsfw: bool,
	#[serde(default)]
	username: String,
	#[serde(default, rename = "baseModel")]
	base_model: String,
	#[serde(default)]
	meta: RawMeta,
}

impl RawImage {
	fn into_post(self) -> Post {
		Post {
			id: self.id,
			tags: build_tags(&self.username, &self.base_model, &self.meta),
			preview_url: self
				.url
				.as_deref()
				.map(to_media_cache_url)
				.unwrap_or_default(),
			width: self.width,
			height: self.height,
			rating: rating_from_nsfw(self.nsfw, &self.nsfw_level),
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: Some(format!("https://civitai.com/images/{}", self.id)),
		}
	}
}

#[derive(Debug, Deserialize, Default)]
struct RawMeta {
	#[serde(default)]
	prompt: Option<String>,
	#[serde(default, rename = "negativePrompt")]
	negative_prompt: Option<String>,
	#[serde(default, rename = "Model")]
	model: Option<String>,
	#[serde(default)]
	sampler: Option<String>,
	#[serde(default)]
	resources: Vec<RawResource>,
}

#[derive(Debug, Deserialize, Default)]
struct RawResource {
	#[serde(default)]
	name: String,
}

impl BooruClient for CivitaiClient {
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
			.items
			.into_iter()
			.map(RawImage::into_post)
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

fn rating_from_nsfw(nsfw: bool, nsfw_level: &str) -> String {
	match nsfw_level.to_ascii_lowercase().as_str() {
		"none" => "s".to_string(),
		"soft" => "q".to_string(),
		"mature" | "x" => "e".to_string(),
		_ => {
			if nsfw {
				"e".to_string()
			} else {
				"s".to_string()
			}
		}
	}
}

fn build_tags(username: &str, base_model: &str, meta: &RawMeta) -> String {
	let mut tags = Vec::new();

	push_unique(&mut tags, username);
	push_unique(&mut tags, base_model);

	if let Some(model) = &meta.model {
		push_unique(&mut tags, model);
	}

	if let Some(sampler) = &meta.sampler {
		push_unique(&mut tags, sampler);
	}

	for resource in &meta.resources {
		push_unique(&mut tags, &resource.name);
	}

	if let Some(prompt) = &meta.prompt {
		push_unique(&mut tags, prompt);
	}

	if let Some(negative_prompt) = &meta.negative_prompt {
		push_unique(&mut tags, negative_prompt);
	}

	tags.join(" ")
}

fn push_unique(tags: &mut Vec<String>, value: &str) {
	let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
	if cleaned.is_empty() {
		return;
	}
	if !tags.iter().any(|existing| existing == &cleaned) {
		tags.push(cleaned);
	}
}

fn to_media_cache_url(original_url: &str) -> String {
	let Some(media_id) = extract_media_id(original_url) else {
		return original_url.to_string();
	};

	format!("{MEDIA_CACHE_BASE_URL}/{media_id}/{MEDIA_CACHE_SIZE_SEGMENT}")
}

fn extract_media_id(url: &str) -> Option<String> {
	let parsed = reqwest::Url::parse(url).ok()?;
	let mut media_id = None;

	for segment in parsed.path_segments()? {
		let normalized = segment.split_once('?').map_or(segment, |(value, _)| value);
		let stem = normalized
			.rsplit_once('.')
			.map_or(normalized, |(value, _)| value);

		if is_uuid_like(stem) {
			media_id = Some(stem.to_string());
		}
	}

	media_id
}

fn is_uuid_like(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 36 {
		return false;
	}

	for (idx, ch) in bytes.iter().enumerate() {
		let is_dash_slot = matches!(idx, 8 | 13 | 18 | 23);
		if is_dash_slot {
			if *ch != b'-' {
				return false;
			}
		} else if !ch.is_ascii_hexdigit() {
			return false;
		}
	}

	true
}

#[cfg(test)]
mod tests {
	use super::{RawImage, to_media_cache_url};

	#[test]
	fn into_post_builds_tags_and_maps_soft_rating() {
		let raw: RawImage = serde_json::from_str(
			r#"{
				"id": 125673839,
				"url": "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/706a7ed9-bbac-4ade-89e1-a40694524396/original=true/706a7ed9-bbac-4ade-89e1-a40694524396.jpeg",
				"width": 840,
				"height": 1080,
				"nsfwLevel": "Soft",
				"nsfw": false,
				"username": "tobycortes",
				"baseModel": "Illustrious",
				"meta": {
					"prompt": "1girl\nbook",
					"negativePrompt": "low quality",
					"Model": "n4mik4_IL_003.fp16",
					"sampler": "LCM",
					"resources": [
						{"name": "Cut3_style"},
						{"name": "DetailedEyes_V3"}
					]
				}
			}"#,
		)
		.expect("valid civitai image json");

		let post = raw.into_post();

		assert_eq!(post.rating, "q");
		assert_eq!(
			post.preview_url,
			"https://image-b2.civitai.com/file/civitai-media-cache/706a7ed9-bbac-4ade-89e1-a40694524396/450x%3Cauto%3E_so"
		);
		assert_eq!(post.width, 840);
		assert_eq!(post.height, 1080);
		assert!(post.tags.contains("tobycortes"));
		assert!(post.tags.contains("Illustrious"));
		assert!(post.tags.contains("n4mik4_IL_003.fp16"));
		assert!(post.tags.contains("LCM"));
		assert!(post.tags.contains("Cut3_style"));
		assert!(post.tags.contains("DetailedEyes_V3"));
		assert!(post.tags.contains("1girl book"));
	}

	#[test]
	fn into_post_falls_back_to_nsfw_boolean_when_level_unknown() {
		let raw: RawImage = serde_json::from_str(
			r#"{
				"id": 77,
				"url": null,
				"width": 0,
				"height": 0,
				"nsfwLevel": "Unknown",
				"nsfw": true,
				"username": "",
				"baseModel": "",
				"meta": {}
			}"#,
		)
		.expect("valid civitai image json");

		let post = raw.into_post();
		assert_eq!(post.rating, "e");
		assert_eq!(post.preview_url, "");
	}

	#[test]
	fn media_cache_url_falls_back_for_unrecognized_path() {
		let original = "https://image.civitai.com/no-uuid-here/sample.jpeg";
		assert_eq!(to_media_cache_url(original), original);
	}
}

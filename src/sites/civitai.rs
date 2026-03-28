use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use super::{BooruClient, Post};
use crate::error::RoobuError;

const POSTS_URL: &str = "https://civitai.com/api/v1/images";
const SITE_NAME: &str = "civitai";
const SITE_NAMESPACE: u64 = 12;
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

	async fn fetch_page_raw(&self, nsfw: bool) -> Result<String, RoobuError> {
		let url = format!("{POSTS_URL}?limit={MAX_POSTS_PER_PAGE}&sort=Newest&nsfw={nsfw}");

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

	fn parse_posts_from_body(&self, body: &str, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		if body.is_empty() || body.starts_with('<') {
			tracing::debug!("empty or HTML response from API, returning empty");
			return Ok(Vec::new());
		}

		let payload: serde_json::Value = serde_json::from_str(body)
			.map_err(|e| RoobuError::Api(format!("JSON parse error: {e}")))?;

		let Some(items) = payload.get("items").and_then(|value| value.as_array()) else {
			tracing::debug!("civitai payload has no items array; returning empty");
			return Ok(Vec::new());
		};

		let mut malformed_items = 0usize;
		let mut posts = Vec::with_capacity(items.len());

		for item in items {
			match serde_json::from_value::<RawImage>(item.clone()) {
				Ok(raw_image) => {
					let post = raw_image.into_post();
					if post.id > last_id {
						posts.push(post);
					}
				}
				Err(error) => {
					malformed_items += 1;
					let post_id = item
						.get("id")
						.and_then(|value| value.as_u64())
						.unwrap_or_default();
					tracing::debug!(post_id, error = %error, "civitai: skipping malformed image item");
				}
			}
		}

		if malformed_items > 0 {
			tracing::warn!(
				malformed_items,
				"civitai: skipped malformed items while parsing response"
			);
		}

		Ok(posts)
	}
}

#[derive(Debug, Deserialize, Default)]
struct RawImage {
	id: u64,
	#[serde(default)]
	url: Option<String>,
	#[serde(default)]
	width: Option<u32>,
	#[serde(default)]
	height: Option<u32>,
	#[serde(default, rename = "nsfwLevel")]
	nsfw_level: Option<String>,
	#[serde(default)]
	nsfw: bool,
	#[serde(default)]
	username: Option<String>,
	#[serde(default, rename = "baseModel")]
	base_model: Option<String>,
	#[serde(default)]
	meta: Option<RawMeta>,
}

impl RawImage {
	fn into_post(self) -> Post {
		let RawImage {
			id,
			url,
			width,
			height,
			nsfw_level,
			nsfw,
			username,
			base_model,
			meta,
		} = self;

		let fallback_page_url = format!("https://civitai.com/images/{id}");
		let preview_url = url
			.and_then(|value| {
				let trimmed = value.trim();
				if trimmed.is_empty() {
					None
				} else {
					Some(trimmed.to_string())
				}
			})
			.unwrap_or_else(|| fallback_page_url.clone());

		let meta = meta.unwrap_or_default();
		let username = username.unwrap_or_default();
		let base_model = base_model.unwrap_or_default();
		let nsfw_level = nsfw_level.unwrap_or_default();

		Post {
			id,
			tags: build_tags(&username, &base_model, &meta),
			preview_url,
			width: width.unwrap_or_default(),
			height: height.unwrap_or_default(),
			rating: rating_from_nsfw(nsfw, &nsfw_level),
			site: SITE_NAME,
			site_namespace: SITE_NAMESPACE,
			canonical_post_url: Some(fallback_page_url),
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
	resources: Option<Vec<RawResource>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawResource {
	#[serde(default)]
	name: Option<String>,
}

impl BooruClient for CivitaiClient {
	fn site_name(&self) -> &'static str {
		SITE_NAME
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		let body = self.fetch_page_raw(true).await?;
		let mut posts = self.parse_posts_from_body(&body, last_id)?;

		posts.retain(|post| post.rating != "s");
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

	if let Some(resources) = &meta.resources {
		for resource in resources {
			if let Some(name) = &resource.name {
				push_unique(&mut tags, name);
			}
		}
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

#[cfg(test)]
mod tests {
	use super::RawImage;

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
			"https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/706a7ed9-bbac-4ade-89e1-a40694524396/original=true/706a7ed9-bbac-4ade-89e1-a40694524396.jpeg"
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
				"meta": null
			}"#,
		)
		.expect("valid civitai image json");

		let post = raw.into_post();
		assert_eq!(post.rating, "e");
		assert_eq!(post.preview_url, "https://civitai.com/images/77");
	}

	#[test]
	fn into_post_accepts_null_meta_and_nullable_strings() {
		let raw: RawImage = serde_json::from_str(
			r#"{
				"id": 88,
				"url": "https://image.civitai.com/example.png",
				"width": 1024,
				"height": 1024,
				"nsfwLevel": null,
				"nsfw": false,
				"username": null,
				"baseModel": null,
				"meta": null
			}"#,
		)
		.expect("valid civitai image json");

		let post = raw.into_post();
		assert_eq!(post.preview_url, "https://image.civitai.com/example.png");
		assert_eq!(post.rating, "s");
		assert_eq!(post.tags, "");
	}

	#[test]
	fn into_post_accepts_null_dimensions_and_null_resources() {
		let raw: RawImage = serde_json::from_str(
			r#"{
				"id": 99,
				"url": "https://image.civitai.com/example2.png",
				"width": null,
				"height": null,
				"nsfwLevel": "None",
				"nsfw": false,
				"username": "artist",
				"baseModel": "model",
				"meta": {
					"resources": null
				}
			}"#,
		)
		.expect("valid civitai image json");

		let post = raw.into_post();
		assert_eq!(post.width, 0);
		assert_eq!(post.height, 0);
		assert!(post.tags.contains("artist"));
		assert!(post.tags.contains("model"));
	}
}

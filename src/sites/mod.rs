pub mod e621;
pub mod kemono;
pub mod rule34;
pub mod safebooru;
pub mod xbooru;

use anyhow::Context;

use crate::config;
use crate::error::RoobuError;

#[derive(Debug, Clone)]
pub struct Post {
	pub id: u64,
	pub tags: String,
	pub preview_url: String,
	pub width: u32,
	pub height: u32,
	pub rating: String,
	pub site: &'static str,
	pub site_namespace: u64,
	pub canonical_post_url: Option<String>,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, Eq, PartialEq)]
pub enum SiteKind {
	Rule34,
	E621,
	Safebooru,
	Xbooru,
	Kemono,
}

pub struct SiteCredentials {
	pub rule34_api_key: Option<String>,
	pub rule34_user_id: Option<String>,
	pub e621_login: Option<String>,
	pub e621_api_key: Option<String>,
	pub kemono_session: Option<String>,
	pub kemono_base_url: Option<String>,
}

pub enum SiteClient {
	Rule34(rule34::Rule34Client),
	E621(e621::E621Client),
	Safebooru(safebooru::SafebooruClient),
	Xbooru(xbooru::XbooruClient),
	Kemono(kemono::KemonoClient),
}

pub fn build_client(site: SiteKind, credentials: SiteCredentials) -> anyhow::Result<SiteClient> {
	match site {
		SiteKind::Rule34 => {
			let api_key = credentials
				.rule34_api_key
				.context("--rule34-api-key (or RULE34_API_KEY) is required when --site rule34")?;
			let user_id = credentials
				.rule34_user_id
				.context("--rule34-user-id (or RULE34_USER_ID) is required when --site rule34")?;
			Ok(SiteClient::Rule34(rule34::Rule34Client::new(
				api_key, user_id,
			)?))
		}
		SiteKind::E621 => Ok(SiteClient::E621(e621::E621Client::new(
			credentials.e621_login,
			credentials.e621_api_key,
		)?)),
		SiteKind::Safebooru => Ok(SiteClient::Safebooru(safebooru::SafebooruClient::new()?)),
		SiteKind::Xbooru => Ok(SiteClient::Xbooru(xbooru::XbooruClient::new()?)),
		SiteKind::Kemono => Ok(SiteClient::Kemono(kemono::KemonoClient::new(
			credentials.kemono_session,
			credentials.kemono_base_url,
		)?)),
	}
}

impl Post {
	pub fn post_url(&self) -> String {
		if let Some(url) = &self.canonical_post_url {
			return url.clone();
		}

		match self.site {
			"rule34" => format!(
				"https://rule34.xxx/index.php?page=post&s=view&id={}",
				self.id
			),
			"e621" => format!("https://e621.net/posts/{}", self.id),
			"safebooru" => format!(
				"https://safebooru.org/index.php?page=post&s=view&id={}",
				self.id
			),
			"xbooru" => format!(
				"https://xbooru.com/index.php?page=post&s=view&id={}",
				self.id
			),
			"kemono" => format!("https://kemono.cr/posts/{}", self.id),
			_ => format!("https://unknown/?id={}", self.id),
		}
	}

	pub fn tags_normalized(&self) -> String {
		let cleaned = self.tags.replace('_', " ");
		let trimmed = cleaned.trim();
		if trimmed.is_empty() {
			"unknown".to_string()
		} else {
			trimmed.to_string()
		}
	}

	pub fn has_preview(&self) -> bool {
		!self.preview_url.is_empty()
	}

	pub fn aspect_ratio_from_dims(w: u32, h: u32) -> Option<f32> {
		if w == 0 && h == 0 {
			return None;
		}
		let (w, h) = (w.max(1) as f32, h.max(1) as f32);
		Some(w.max(h) / w.min(h))
	}

	pub fn aspect_ratio(&self) -> Option<f32> {
		Self::aspect_ratio_from_dims(self.width, self.height)
	}

	pub fn is_aspect_ratio_ok(ratio: f32) -> bool {
		ratio <= config::MAX_IMAGE_ASPECT_RATIO
	}

	pub fn passes_preflight(&self) -> bool {
		if !self.has_preview() {
			return false;
		}
		if let Some(ratio) = self.aspect_ratio()
			&& !Self::is_aspect_ratio_ok(ratio)
		{
			tracing::debug!(post_id = self.id, ratio, "skipped: aspect ratio");
			return false;
		}
		true
	}
}

pub trait BooruClient: Send + Sync {
	fn site_name(&self) -> &'static str;

	fn fetch_recent(
		&self,
		last_id: u64,
	) -> impl Future<Output = Result<Vec<Post>, RoobuError>> + Send;
	fn download_preview(
		&self,
		url: &str,
	) -> impl Future<Output = Result<bytes::Bytes, RoobuError>> + Send;
}

impl BooruClient for SiteClient {
	fn site_name(&self) -> &'static str {
		match self {
			SiteClient::Rule34(client) => client.site_name(),
			SiteClient::E621(client) => client.site_name(),
			SiteClient::Safebooru(client) => client.site_name(),
			SiteClient::Xbooru(client) => client.site_name(),
			SiteClient::Kemono(client) => client.site_name(),
		}
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		match self {
			SiteClient::Rule34(client) => client.fetch_recent(last_id).await,
			SiteClient::E621(client) => client.fetch_recent(last_id).await,
			SiteClient::Safebooru(client) => client.fetch_recent(last_id).await,
			SiteClient::Xbooru(client) => client.fetch_recent(last_id).await,
			SiteClient::Kemono(client) => client.fetch_recent(last_id).await,
		}
	}

	async fn download_preview(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
		match self {
			SiteClient::Rule34(client) => client.download_preview(url).await,
			SiteClient::E621(client) => client.download_preview(url).await,
			SiteClient::Safebooru(client) => client.download_preview(url).await,
			SiteClient::Xbooru(client) => client.download_preview(url).await,
			SiteClient::Kemono(client) => client.download_preview(url).await,
		}
	}
}

use std::future::Future;

pub fn validate_downloaded_image(post_id: u64, bytes: &[u8]) -> Option<image::DynamicImage> {
	if bytes.len() < config::MIN_DOWNLOADED_IMAGE_BYTES {
		tracing::warn!(post_id, len = bytes.len(), "skipped: tiny image");
		return None;
	}

	let img = match image::load_from_memory(bytes) {
		Ok(img) => img,
		Err(e) => {
			tracing::warn!(post_id, error = %e, "skipped: decode failed");
			return None;
		}
	};

	let (w, h) = (img.width(), img.height());
	if w < config::MIN_IMAGE_EDGE_PX || h < config::MIN_IMAGE_EDGE_PX {
		tracing::warn!(post_id, w, h, "skipped: too small");
		return None;
	}

	if let Some(ratio) = Post::aspect_ratio_from_dims(w, h)
		&& !Post::is_aspect_ratio_ok(ratio)
	{
		tracing::debug!(post_id, ratio, "skipped: decoded aspect ratio");
		return None;
	}

	Some(img)
}

#[cfg(test)]
mod tests {
	use super::Post;

	#[test]
	fn post_url_is_site_specific() {
		let rule34 = Post {
			id: 123,
			tags: String::new(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "rule34",
			site_namespace: 1,
			canonical_post_url: None,
		};
		let e621 = Post {
			id: 456,
			tags: String::new(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "e621",
			site_namespace: 2,
			canonical_post_url: None,
		};
		let safebooru = Post {
			id: 789,
			tags: String::new(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "safebooru",
			site_namespace: 3,
			canonical_post_url: None,
		};
		let xbooru = Post {
			id: 321,
			tags: String::new(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "xbooru",
			site_namespace: 6,
			canonical_post_url: None,
		};
		let kemono = Post {
			id: 654,
			tags: String::new(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "kemono",
			site_namespace: 7,
			canonical_post_url: None,
		};

		assert_eq!(
			rule34.post_url(),
			"https://rule34.xxx/index.php?page=post&s=view&id=123"
		);
		assert_eq!(e621.post_url(), "https://e621.net/posts/456");
		assert_eq!(
			safebooru.post_url(),
			"https://safebooru.org/index.php?page=post&s=view&id=789"
		);
		assert_eq!(
			xbooru.post_url(),
			"https://xbooru.com/index.php?page=post&s=view&id=321"
		);
		assert_eq!(kemono.post_url(), "https://kemono.cr/posts/654");
	}

	#[test]
	fn tags_normalized_uses_unknown_for_empty_input() {
		let post = Post {
			id: 1,
			tags: "   ".to_string(),
			preview_url: String::new(),
			width: 0,
			height: 0,
			rating: String::new(),
			site: "rule34",
			site_namespace: 1,
			canonical_post_url: None,
		};

		assert_eq!(post.tags_normalized(), "unknown");
	}
}

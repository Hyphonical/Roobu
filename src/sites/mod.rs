//! Booru site adapters for fetching and parsing image posts.
//!
//! Each site implements the [`BooruClient`] trait, providing a uniform interface
//! for fetching recent posts and downloading thumbnails. The [`SiteClient`] enum
//! dispatches calls to the appropriate adapter at runtime.

pub mod aibooru;
pub mod civitai;
mod common;
pub mod danbooru;
pub mod e621;
pub mod e6ai;
pub mod gelbooru;
mod http_client;
pub mod kemono;
pub mod konachan;
pub mod rule34;
pub mod safebooru;
pub mod xbooru;
pub mod yandere;

use anyhow::Context;

use crate::config;
use crate::error::RoobuError;

// ── Data Types ──────────────────────────────────────────────────────────────

/// A normalized image post from any supported site.
#[derive(Debug, Clone)]
pub struct Post {
	/// Unique post ID within the site.
	pub id: u64,
	/// Space-separated tags describing the image content.
	pub tags: String,
	/// URL to a small thumbnail for quick preview/download.
	pub thumbnail_url: String,
	/// Optional URL to the full-resolution image.
	pub direct_image_url: Option<String>,
	/// Image width in pixels.
	pub width: u32,
	/// Image height in pixels.
	pub height: u32,
	/// Content rating (e.g., "s", "q", "e").
	pub rating: String,
	/// Human-readable site name (e.g., "rule34", "e621").
	pub site: &'static str,
	/// Numeric namespace used for encoding point IDs in Qdrant.
	pub site_namespace: u64,
	/// Optional canonical URL to the post page on the site.
	pub canonical_post_url: Option<String>,
}

/// Supported booru sites, used for CLI argument parsing and client dispatch.
#[derive(clap::ValueEnum, Debug, Clone, Copy, Eq, PartialEq)]
pub enum SiteKind {
	Rule34,
	E621,
	Safebooru,
	Xbooru,
	Kemono,
	Aibooru,
	Danbooru,
	Civitai,
	#[value(name = "e6ai")]
	E6Ai,
	Gelbooru,
	Konachan,
	Yandere,
}

/// Credentials required for sites that need authentication.
pub struct SiteCredentials {
	pub rule34_api_key: Option<String>,
	pub rule34_user_id: Option<String>,
	pub e621_login: Option<String>,
	pub e621_api_key: Option<String>,
	pub gelbooru_api_key: Option<String>,
	pub gelbooru_user_id: Option<String>,
	pub kemono_session: Option<String>,
	pub kemono_base_url: Option<String>,
}

/// Enum dispatching to the appropriate site adapter.
pub enum SiteClient {
	Rule34(rule34::Rule34Client),
	E621(e621::E621Client),
	Safebooru(safebooru::SafebooruClient),
	Xbooru(xbooru::XbooruClient),
	Kemono(kemono::KemonoClient),
	Aibooru(aibooru::AibooruClient),
	Danbooru(danbooru::DanbooruClient),
	Civitai(civitai::CivitaiClient),
	E6Ai(e6ai::E6AiClient),
	Gelbooru(gelbooru::GelbooruClient),
	Konachan(konachan::KonachanClient),
	Yandere(yandere::YandereClient),
}

// ── Client Factory ──────────────────────────────────────────────────────────

/// Build a site client for the given site kind and credentials.
///
/// # Errors
/// Returns an error if required credentials are missing for the site,
/// or if the HTTP client fails to initialize.
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
		SiteKind::Aibooru => Ok(SiteClient::Aibooru(aibooru::AibooruClient::new()?)),
		SiteKind::Danbooru => Ok(SiteClient::Danbooru(danbooru::DanbooruClient::new()?)),
		SiteKind::Civitai => Ok(SiteClient::Civitai(civitai::CivitaiClient::new()?)),
		SiteKind::E6Ai => Ok(SiteClient::E6Ai(e6ai::E6AiClient::new()?)),
		SiteKind::Gelbooru => {
			let api_key = credentials.gelbooru_api_key.context(
				"--gelbooru-api-key (or GELBOORU_API_KEY) is required when --site gelbooru",
			)?;
			let user_id = credentials.gelbooru_user_id.context(
				"--gelbooru-user-id (or GELBOORU_USER_ID) is required when --site gelbooru",
			)?;
			Ok(SiteClient::Gelbooru(gelbooru::GelbooruClient::new(
				api_key, user_id,
			)?))
		}
		SiteKind::Konachan => Ok(SiteClient::Konachan(konachan::KonachanClient::new()?)),
		SiteKind::Yandere => Ok(SiteClient::Yandere(yandere::YandereClient::new()?)),
	}
}

// ── Post Helpers ────────────────────────────────────────────────────────────

impl Post {
	/// Construct the canonical URL to the post page on its source site.
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
			"e6ai" => format!("https://e6ai.net/posts/{}", self.id),
			"gelbooru" => format!(
				"https://gelbooru.com/index.php?page=post&s=view&id={}",
				self.id
			),
			"safebooru" => format!(
				"https://safebooru.org/index.php?page=post&s=view&id={}",
				self.id
			),
			"xbooru" => format!(
				"https://xbooru.com/index.php?page=post&s=view&id={}",
				self.id
			),
			"kemono" => format!("https://kemono.cr/posts/{}", self.id),
			"aibooru" => format!("https://aibooru.online/posts/{}", self.id),
			"danbooru" => format!("https://danbooru.donmai.us/posts/{}", self.id),
			"civitai" => format!("https://civitai.com/images/{}", self.id),
			"konachan" => format!("https://konachan.com/post/show/{}", self.id),
			"yandere" => format!("https://yande.re/post/show/{}", self.id),
			_ => format!("https://unknown/?id={}", self.id),
		}
	}

	/// Check if the post has a non-empty thumbnail URL.
	pub fn has_thumbnail(&self) -> bool {
		!self.thumbnail_url.is_empty()
	}

	/// Return the direct image URL if available, falling back to the thumbnail URL.
	pub fn preferred_image_url(&self) -> String {
		self.direct_image_url
			.clone()
			.unwrap_or_else(|| self.thumbnail_url.clone())
	}

	/// Compute the aspect ratio (longer edge / shorter edge) from dimensions.
	///
	/// Returns `None` if both dimensions are zero.
	pub fn aspect_ratio_from_dims(w: u32, h: u32) -> Option<f32> {
		if w == 0 && h == 0 {
			return None;
		}
		let (w, h) = (w.max(1) as f32, h.max(1) as f32);
		Some(w.max(h) / w.min(h))
	}

	/// Compute the aspect ratio from the post's dimensions.
	pub fn aspect_ratio(&self) -> Option<f32> {
		Self::aspect_ratio_from_dims(self.width, self.height)
	}

	/// Check if the aspect ratio is within acceptable bounds.
	pub fn is_aspect_ratio_ok(ratio: f32) -> bool {
		ratio <= config::MAX_IMAGE_ASPECT_RATIO
	}

	/// Run preflight checks to determine if this post is worth processing.
	///
	/// Currently checks for a valid thumbnail URL and acceptable aspect ratio.
	pub fn passes_preflight(&self) -> bool {
		if !self.has_thumbnail() {
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

// ── BooruClient Trait ───────────────────────────────────────────────────────

/// Trait for site adapters, providing a uniform interface for fetching posts.
pub trait BooruClient: Send + Sync {
	/// Return the human-readable site name.
	fn site_name(&self) -> &'static str;

	/// Fetch recent posts newer than the given post ID.
	///
	/// Returns posts sorted by ID ascending (oldest first), which allows the
	/// ingest loop to process them in order and update the checkpoint correctly.
	fn fetch_recent(
		&self,
		last_id: u64,
	) -> impl std::future::Future<Output = Result<Vec<Post>, RoobuError>> + Send;

	/// Download thumbnail bytes from the given URL.
	fn download_thumbnail(
		&self,
		url: &str,
	) -> impl std::future::Future<Output = Result<bytes::Bytes, RoobuError>> + Send;
}

/// Validate downloaded image bytes and decode into a [`DynamicImage`].
///
/// Checks minimum file size and attempts to decode the image. Returns `None`
/// if the data is too small or cannot be decoded as a supported image format.
pub fn validate_downloaded_image(post_id: u64, data: &[u8]) -> Option<image::DynamicImage> {
	if data.len() < config::MIN_DOWNLOADED_IMAGE_BYTES {
		tracing::debug!(post_id, size = data.len(), "skipped: too small");
		return None;
	}

	match image::load_from_memory(data) {
		Ok(img) => {
			let (w, h) = (img.width(), img.height());
			if w < config::MIN_IMAGE_EDGE_PX || h < config::MIN_IMAGE_EDGE_PX {
				tracing::debug!(post_id, w, h, "skipped: below minimum dimensions");
				return None;
			}
			Some(img)
		}
		Err(e) => {
			tracing::debug!(post_id, error = %e, "skipped: failed to decode image");
			None
		}
	}
}

/// Delegate [`BooruClient`] calls through the [`SiteClient`] enum.
impl BooruClient for SiteClient {
	fn site_name(&self) -> &'static str {
		match self {
			Self::Rule34(c) => c.site_name(),
			Self::E621(c) => c.site_name(),
			Self::Safebooru(c) => c.site_name(),
			Self::Xbooru(c) => c.site_name(),
			Self::Kemono(c) => c.site_name(),
			Self::Aibooru(c) => c.site_name(),
			Self::Danbooru(c) => c.site_name(),
			Self::Civitai(c) => c.site_name(),
			Self::E6Ai(c) => c.site_name(),
			Self::Gelbooru(c) => c.site_name(),
			Self::Konachan(c) => c.site_name(),
			Self::Yandere(c) => c.site_name(),
		}
	}

	async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
		match self {
			Self::Rule34(c) => c.fetch_recent(last_id).await,
			Self::E621(c) => c.fetch_recent(last_id).await,
			Self::Safebooru(c) => c.fetch_recent(last_id).await,
			Self::Xbooru(c) => c.fetch_recent(last_id).await,
			Self::Kemono(c) => c.fetch_recent(last_id).await,
			Self::Aibooru(c) => c.fetch_recent(last_id).await,
			Self::Danbooru(c) => c.fetch_recent(last_id).await,
			Self::Civitai(c) => c.fetch_recent(last_id).await,
			Self::E6Ai(c) => c.fetch_recent(last_id).await,
			Self::Gelbooru(c) => c.fetch_recent(last_id).await,
			Self::Konachan(c) => c.fetch_recent(last_id).await,
			Self::Yandere(c) => c.fetch_recent(last_id).await,
		}
	}

	async fn download_thumbnail(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
		match self {
			Self::Rule34(c) => c.download_thumbnail(url).await,
			Self::E621(c) => c.download_thumbnail(url).await,
			Self::Safebooru(c) => c.download_thumbnail(url).await,
			Self::Xbooru(c) => c.download_thumbnail(url).await,
			Self::Kemono(c) => c.download_thumbnail(url).await,
			Self::Aibooru(c) => c.download_thumbnail(url).await,
			Self::Danbooru(c) => c.download_thumbnail(url).await,
			Self::Civitai(c) => c.download_thumbnail(url).await,
			Self::E6Ai(c) => c.download_thumbnail(url).await,
			Self::Gelbooru(c) => c.download_thumbnail(url).await,
			Self::Konachan(c) => c.download_thumbnail(url).await,
			Self::Yandere(c) => c.download_thumbnail(url).await,
		}
	}
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
	use super::Post;

	fn make_post(id: u64, site: &'static str, site_namespace: u64) -> Post {
		Post {
			id,
			tags: String::new(),
			thumbnail_url: String::new(),
			direct_image_url: None,
			width: 0,
			height: 0,
			rating: String::new(),
			site,
			site_namespace,
			canonical_post_url: None,
		}
	}

	#[test]
	fn post_url_is_site_specific() {
		let rule34 = make_post(123, "rule34", 1);
		let e621 = make_post(456, "e621", 2);
		let safebooru = make_post(789, "safebooru", 3);
		let xbooru = make_post(321, "xbooru", 6);
		let kemono = make_post(654, "kemono", 7);
		let aibooru = make_post(777, "aibooru", 8);
		let danbooru = make_post(888, "danbooru", 5);
		let civitai = make_post(333, "civitai", 12);
		let e6ai = make_post(999, "e6ai", 9);
		let gelbooru = make_post(444, "gelbooru", 4);
		let konachan = make_post(111, "konachan", 10);
		let yandere = make_post(222, "yandere", 11);

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
		assert_eq!(aibooru.post_url(), "https://aibooru.online/posts/777");
		assert_eq!(danbooru.post_url(), "https://danbooru.donmai.us/posts/888");
		assert_eq!(civitai.post_url(), "https://civitai.com/images/333");
		assert_eq!(e6ai.post_url(), "https://e6ai.net/posts/999");
		assert_eq!(
			gelbooru.post_url(),
			"https://gelbooru.com/index.php?page=post&s=view&id=444"
		);
		assert_eq!(konachan.post_url(), "https://konachan.com/post/show/111");
		assert_eq!(yandere.post_url(), "https://yande.re/post/show/222");
	}
}

pub mod rule34;

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
}

impl Post {
	pub fn post_url(&self) -> String {
		match self.site {
			"rule34" => format!(
				"https://rule34.xxx/index.php?page=post&s=view&id={}",
				self.id
			),
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
		ratio <= 2.0
	}

	pub fn passes_preflight(&self) -> bool {
		if !self.has_preview() {
			return false;
		}
		if let Some(ratio) = self.aspect_ratio() {
			if !Self::is_aspect_ratio_ok(ratio) {
				tracing::debug!(post_id = self.id, ratio, "skipped: aspect ratio");
				return false;
			}
		}
		true
	}
}

pub trait BooruClient: Send + Sync {
	fn site_name(&self) -> &'static str;
	fn site_namespace(&self) -> u64;

	fn fetch_recent(
		&self,
		last_id: u64,
	) -> impl Future<Output = Result<Vec<Post>, RoobuError>> + Send;
	fn download_preview(
		&self,
		url: &str,
	) -> impl Future<Output = Result<bytes::Bytes, RoobuError>> + Send;
}

use std::future::Future;

pub fn validate_downloaded_image(post_id: u64, bytes: &[u8]) -> Option<image::DynamicImage> {
	if bytes.len() < 500 {
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
	if w < 32 || h < 32 {
		tracing::warn!(post_id, w, h, "skipped: too small");
		return None;
	}

	if let Some(ratio) = Post::aspect_ratio_from_dims(w, h) {
		if !Post::is_aspect_ratio_ok(ratio) {
			tracing::debug!(post_id, ratio, "skipped: decoded aspect ratio");
			return None;
		}
	}

	Some(img)
}

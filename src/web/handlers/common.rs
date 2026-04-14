use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

/// Standard API response envelope.
///
/// All successful responses are wrapped in this structure to provide
/// consistent pagination metadata and a uniform client interface.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T> {
	/// The response payload.
	pub data: T,
	/// Pagination metadata (present for list endpoints).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub meta: Option<ResponseMeta>,
}

/// Pagination and query metadata.
#[derive(Debug, Serialize, ToSchema)]
pub struct ResponseMeta {
	/// Number of items returned in this response.
	pub count: usize,
	/// Offset for the next page (None if no more results).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub next_offset: Option<u64>,
}

impl<T: Serialize> ApiResponse<T> {
	pub fn ok(data: T) -> Self {
		Self { data, meta: None }
	}

	pub fn list(data: T, count: usize, next_offset: Option<u64>) -> Self {
		Self {
			data,
			meta: Some(ResponseMeta { count, next_offset }),
		}
	}
}

/// A post with full metadata.
///
/// Used across all endpoints that return post data (search, recent,
/// similar, single post). The `score` field is only populated for search
/// and similarity results.
#[derive(Debug, Serialize, ToSchema)]
pub struct PostDto {
	/// Site-local post ID.
	pub post_id: u64,
	/// Site name this post belongs to (e.g. `"rule34"`, `"e621"`).
	pub site: String,
	/// Canonical URL to the post page on the source site.
	pub post_url: String,
	/// URL to a small thumbnail image.
	pub thumbnail_url: String,
	/// URL to the full-resolution image (falls back to thumbnail if unavailable).
	pub direct_image_url: String,
	/// Image width in pixels (0 if unknown).
	pub width: u32,
	/// Image height in pixels (0 if unknown).
	pub height: u32,
	/// Unix timestamp of when the post was ingested.
	pub ingestion_date: i64,
	/// Cosine similarity score in [0.0, 1.0]; only set for search/similar results.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub score: Option<f32>,
	/// Space-separated tags describing the image content.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tags: Option<String>,
	/// Content rating (e.g. `"s"`, `"q"`, `"e"`).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rating: Option<String>,
}

impl PostDto {
	/// Create a PostDto from a SearchResult with an optional score.
	pub fn from_result(r: crate::store::SearchResult, score: Option<f32>) -> Self {
		let tags = if r.tags.is_empty() {
			None
		} else {
			Some(r.tags)
		};
		let rating = if r.rating.is_empty() {
			None
		} else {
			Some(r.rating)
		};
		Self {
			post_id: r.post_id,
			site: r.site,
			post_url: r.post_url,
			thumbnail_url: r.thumbnail_url,
			direct_image_url: r.direct_image_url,
			width: r.width,
			height: r.height,
			ingestion_date: r.ingestion_date,
			score,
			tags,
			rating,
		}
	}
}

/// Error response body.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDto {
	pub error: String,
}

pub(super) fn bad_request(msg: impl Into<String>) -> axum::response::Response {
	(
		StatusCode::BAD_REQUEST,
		Json(ApiResponse::ok(ErrorDto { error: msg.into() })),
	)
		.into_response()
}

pub(super) fn internal_error(msg: impl Into<String>) -> axum::response::Response {
	(
		StatusCode::INTERNAL_SERVER_ERROR,
		Json(ApiResponse::ok(ErrorDto { error: msg.into() })),
	)
		.into_response()
}

pub(super) fn not_found(msg: impl Into<String>) -> axum::response::Response {
	(
		StatusCode::NOT_FOUND,
		Json(ApiResponse::ok(ErrorDto { error: msg.into() })),
	)
		.into_response()
}

/// Wrap a response with `Cache-Control: no-store` to prevent browser/proxy caching.
pub(super) fn no_cache(resp: axum::response::Response) -> axum::response::Response {
	let (mut parts, body) = resp.into_parts();
	parts.headers.insert(
		"Cache-Control",
		axum::http::HeaderValue::from_static("no-store, no-cache, must-revalidate"),
	);
	axum::response::Response::from_parts(parts, body)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SiteInput {
	One(String),
	Many(Vec<String>),
}

/// Deserialize `site` values from a single string, repeated params, or comma-separated values.
pub(super) fn deserialize_sites<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
	D: Deserializer<'de>,
{
	let raw_values = match SiteInput::deserialize(deserializer)? {
		SiteInput::One(value) => vec![value],
		SiteInput::Many(values) => values,
	};

	Ok(raw_values
		.into_iter()
		.flat_map(|value| {
			value
				.split(',')
				.map(str::trim)
				.filter(|s| !s.is_empty())
				.map(ToOwned::to_owned)
				.collect::<Vec<_>>()
		})
		.collect())
}

/// Parse sites into normalized non-empty values.
pub(super) fn parse_sites(raw: &[String]) -> Vec<String> {
	raw.iter()
		.flat_map(|s| s.split(','))
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(ToOwned::to_owned)
		.collect()
}

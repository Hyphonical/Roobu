use axum::{
	Json,
	extract::{Multipart, Path, Query, State},
	response::IntoResponse,
};
use serde::Deserialize;
use utoipa::ToSchema;

use super::common::{
	ApiResponse, ErrorDto, PostDto, bad_request, deserialize_sites, internal_error, no_cache,
	not_found, parse_sites,
};
use crate::commands::search::{SearchRequest, execute_search};
use crate::web::state::AppState;

const MAX_UPLOAD_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Query parameters for the `/api/search` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchParams {
	/// Text query for semantic search.
	#[schema(example = "fluffy cat sitting on a windowsill")]
	pub q: Option<String>,

	/// Publicly accessible image URL to use as a visual query.
	#[schema(example = "https://example.com/image.jpg")]
	pub image_url: Option<String>,

	/// Sites to restrict results to. Repeated param or comma-separated.
	/// Omit to search across all indexed sites.
	#[schema(example = json!(["rule34", "e621"]))]
	#[serde(default, deserialize_with = "deserialize_sites")]
	pub site: Vec<String>,

	/// Maximum number of results to return (default: 10, max: 100).
	#[schema(example = 10, minimum = 1, maximum = 100)]
	pub limit: Option<u64>,

	/// Image weight for hybrid text+image queries, in [0.0, 1.0].
	#[schema(example = 0.5, minimum = 0.0, maximum = 1.0)]
	pub image_weight: Option<f32>,
}

/// Query parameters for the `/api/search/similar/{site}/{post_id}` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SimilarParams {
	/// Maximum results to return (default 10, max 100).
	#[schema(example = 10, minimum = 1, maximum = 100)]
	pub limit: Option<u64>,
}

/// Multipart form fields for `/api/search/upload`.
#[allow(dead_code)]
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchUploadForm {
	/// Text query for semantic search.
	#[schema(example = "fluffy cat sitting on a windowsill")]
	pub q: Option<String>,

	/// Uploaded image file used as visual query.
	#[schema(format = Binary, nullable = true)]
	pub image: Option<String>,

	/// Sites to restrict results to. Comma-separated.
	#[schema(example = "rule34,e621")]
	pub site: Option<String>,

	/// Maximum number of results to return (default: 10, max: 100).
	#[schema(example = 10, minimum = 1, maximum = 100)]
	pub limit: Option<u64>,

	/// Image weight for hybrid text+image queries, in [0.0, 1.0].
	#[schema(example = 0.5, minimum = 0.0, maximum = 1.0)]
	pub image_weight: Option<f32>,
}

/// Semantic search over indexed images.
#[utoipa::path(
	get,
	path = "/api/search",
	params(
		("q" = Option<String>, Query, description = "Text query for semantic search"),
		("image_url" = Option<String>, Query, description = "Publicly accessible image URL for visual search"),
		("site" = Option<Vec<String>>, Query, description = "Sites to filter by (repeated or comma-separated)"),
		("limit" = Option<u64>, Query, description = "Maximum results (default 10, max 100)"),
		("image_weight" = Option<f32>, Query, description = "Image weight for hybrid queries [0.0-1.0]"),
	),
	responses(
		(status = 200, description = "Search results", body = ApiResponse<Vec<PostDto>>),
		(status = 400, description = "Bad request", body = ApiResponse<ErrorDto>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "search"
)]
pub async fn search(
	Query(params): Query<SearchParams>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	let has_text = params.q.as_deref().is_some_and(|q| !q.trim().is_empty());
	let has_image = params
		.image_url
		.as_deref()
		.is_some_and(|u| !u.trim().is_empty());

	if !has_text && !has_image {
		return bad_request("Provide at least one of: 'q' (text query) or 'image_url'");
	}

	let limit = params.limit.unwrap_or(10).min(100);
	let image_weight = params.image_weight.unwrap_or(0.5).clamp(0.0, 1.0);
	let sites = parse_sites(&params.site);

	let image_bytes = if has_image {
		let url = params.image_url.as_deref().unwrap_or_default();
		match download_image_bytes(url).await {
			Ok(bytes) => Some(bytes),
			Err(e) => {
				tracing::warn!(url, error = %e, "failed to download query image");
				return bad_request(format!("Failed to download image_url: {e}"));
			}
		}
	} else {
		None
	};

	let request = SearchRequest {
		text_query: if has_text {
			params.q.map(|q| q.trim().to_owned())
		} else {
			None
		},
		image_path: None,
		image_bytes,
		limit,
		site_filter: sites,
		image_weight,
	};

	match execute_search(request, &state.embedder, &state.store).await {
		Ok(response) => {
			let dtos = to_scored_post_dtos(response.results);
			let count = dtos.len();
			no_cache(Json(ApiResponse::list(dtos, count, None)).into_response())
		}
		Err(e) => {
			tracing::error!(error = %e, "search failed");
			internal_error(format!("Search failed: {e}"))
		}
	}
}

/// Semantic search using multipart form-data, including direct image upload.
#[utoipa::path(
	post,
	path = "/api/search/upload",
	request_body(
		content = SearchUploadForm,
		content_type = "multipart/form-data",
		description = "Multipart form with optional q text, optional image file, optional comma-separated site, limit, and image_weight"
	),
	responses(
		(status = 200, description = "Search results", body = ApiResponse<Vec<PostDto>>),
		(status = 400, description = "Bad request", body = ApiResponse<ErrorDto>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "search"
)]
pub async fn search_upload(
	State(state): State<AppState>,
	mut multipart: Multipart,
) -> impl IntoResponse {
	let mut text_query: Option<String> = None;
	let mut image_bytes: Option<Vec<u8>> = None;
	let mut raw_sites: Vec<String> = Vec::new();
	let mut limit: Option<u64> = None;
	let mut image_weight: Option<f32> = None;

	loop {
		let next_field = match multipart.next_field().await {
			Ok(field) => field,
			Err(error) => return bad_request(format!("Invalid multipart payload: {error}")),
		};

		let Some(field) = next_field else {
			break;
		};

		let name = field.name().unwrap_or_default().to_owned();
		match name.as_str() {
			"q" => {
				let value = match field.text().await {
					Ok(value) => value,
					Err(error) => return bad_request(format!("Invalid q field: {error}")),
				};
				text_query = Some(value);
			}
			"site" => {
				let value = match field.text().await {
					Ok(value) => value,
					Err(error) => return bad_request(format!("Invalid site field: {error}")),
				};
				raw_sites.push(value);
			}
			"limit" => {
				let value = match field.text().await {
					Ok(value) => value,
					Err(error) => return bad_request(format!("Invalid limit field: {error}")),
				};
				match value.trim().parse::<u64>() {
					Ok(parsed) => limit = Some(parsed),
					Err(_) => return bad_request("limit must be a positive integer"),
				}
			}
			"image_weight" => {
				let value = match field.text().await {
					Ok(value) => value,
					Err(error) => {
						return bad_request(format!("Invalid image_weight field: {error}"));
					}
				};
				match value.trim().parse::<f32>() {
					Ok(parsed) => image_weight = Some(parsed),
					Err(_) => {
						return bad_request("image_weight must be a number between 0.0 and 1.0");
					}
				}
			}
			"image" => {
				let bytes = match field.bytes().await {
					Ok(bytes) => bytes,
					Err(error) => return bad_request(format!("Invalid image field: {error}")),
				};

				if bytes.is_empty() {
					return bad_request("image upload is empty");
				}

				if bytes.len() > MAX_UPLOAD_IMAGE_BYTES {
					return bad_request(format!(
						"image upload exceeds {} bytes",
						MAX_UPLOAD_IMAGE_BYTES
					));
				}

				image_bytes = Some(bytes.to_vec());
			}
			_ => {
				// Unknown fields are ignored to keep backward compatibility.
			}
		}
	}

	let text_query = text_query
		.as_deref()
		.map(str::trim)
		.filter(|q| !q.is_empty())
		.map(ToOwned::to_owned);

	if text_query.is_none() && image_bytes.is_none() {
		return bad_request("Provide at least one of: q text field or image file field");
	}

	let request = SearchRequest {
		text_query,
		image_path: None,
		image_bytes,
		limit: limit.unwrap_or(10).clamp(1, 100),
		site_filter: parse_sites(&raw_sites),
		image_weight: image_weight.unwrap_or(0.5).clamp(0.0, 1.0),
	};

	match execute_search(request, &state.embedder, &state.store).await {
		Ok(response) => {
			let dtos = to_scored_post_dtos(response.results);
			let count = dtos.len();
			no_cache(Json(ApiResponse::list(dtos, count, None)).into_response())
		}
		Err(e) => {
			tracing::error!(error = %e, "search upload failed");
			internal_error(format!("Search upload failed: {e}"))
		}
	}
}

/// Find visually similar images to a known post.
#[utoipa::path(
	get,
	path = "/api/search/similar/{site}/{post_id}",
	params(
		("site" = String, Path, description = "Site name of the target post"),
		("post_id" = u64, Path, description = "Site-local post ID of the target"),
		("limit" = Option<u64>, Query, description = "Maximum results (default 10, max 100)"),
	),
	responses(
		(status = 200, description = "Similar posts", body = ApiResponse<Vec<PostDto>>),
		(status = 404, description = "Target post not found", body = ApiResponse<ErrorDto>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "search"
)]
pub async fn search_similar(
	Path((site, post_id)): Path<(String, u64)>,
	Query(params): Query<SimilarParams>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	let limit = params.limit.unwrap_or(10).clamp(1, 100);

	match state.store.search_similar(&site, post_id, limit).await {
		Ok(results) => {
			let dtos = to_scored_post_dtos(results);
			let count = dtos.len();
			no_cache(Json(ApiResponse::list(dtos, count, None)).into_response())
		}
		Err(e) => {
			if e.to_string().contains("not found") {
				not_found(format!("Target post {site}/{post_id} not found"))
			} else {
				tracing::error!(error = %e, "similar search failed");
				internal_error(format!("Similar search failed: {e}"))
			}
		}
	}
}

/// Download an image from a URL into memory.
async fn download_image_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
	let bytes = reqwest::get(url)
		.await
		.map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?
		.error_for_status()
		.map_err(|e| anyhow::anyhow!("HTTP error: {e}"))?
		.bytes()
		.await
		.map_err(|e| anyhow::anyhow!("Failed to read response body: {e}"))?;

	Ok(bytes.to_vec())
}

fn to_scored_post_dtos(results: Vec<crate::store::SearchResult>) -> Vec<PostDto> {
	results
		.into_iter()
		.map(|result| {
			let score = result.score;
			PostDto::from_result(result, Some(score))
		})
		.collect()
}

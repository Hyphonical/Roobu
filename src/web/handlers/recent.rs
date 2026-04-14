use axum::{
	Json,
	extract::{Query, State},
	response::IntoResponse,
};
use serde::Deserialize;
use utoipa::ToSchema;

use super::common::{
	ApiResponse, ErrorDto, PostDto, deserialize_sites, internal_error, no_cache, parse_sites,
};
use crate::web::state::AppState;

/// Query parameters for the `/api/recent` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RecentParams {
	/// Number of results to return (default 20, max 100).
	#[schema(example = 20, minimum = 1, maximum = 100)]
	pub limit: Option<u64>,

	/// Sites to restrict results to. Repeated param or comma-separated.
	#[schema(example = json!(["rule34", "e621"]))]
	#[serde(default, deserialize_with = "deserialize_sites")]
	pub site: Vec<String>,

	/// Pagination offset (default: 0).
	#[schema(example = 0, minimum = 0)]
	pub offset: Option<u64>,
}

/// Get the most recently ingested posts.
#[utoipa::path(
	get,
	path = "/api/recent",
	params(
		("limit" = Option<u64>, Query, description = "Number of results (default 20, max 100)"),
		("site" = Option<Vec<String>>, Query, description = "Sites to filter by (repeated or comma-separated)"),
		("offset" = Option<u64>, Query, description = "Pagination offset"),
	),
	responses(
		(status = 200, description = "Recent posts", body = ApiResponse<Vec<PostDto>>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "recent"
)]
pub async fn recent(
	Query(params): Query<RecentParams>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	let limit = params.limit.unwrap_or(20).clamp(1, 100);
	let offset = params.offset.unwrap_or(0);
	let sites = parse_sites(&params.site);
	let site_refs: Vec<&str> = sites.iter().map(String::as_str).collect();

	match state.store.fetch_recent(limit, offset, &site_refs).await {
		Ok(page) => {
			let has_more = page.has_more;
			let next_offset = if has_more { Some(offset + limit) } else { None };
			let dtos: Vec<PostDto> = page
				.posts
				.into_iter()
				.map(|r| PostDto::from_result(r, None))
				.collect();
			let count = dtos.len();
			no_cache(Json(ApiResponse::list(dtos, count, next_offset)).into_response())
		}
		Err(e) => {
			tracing::error!(error = %e, "recent fetch failed");
			internal_error(format!("Recent fetch failed: {e}"))
		}
	}
}

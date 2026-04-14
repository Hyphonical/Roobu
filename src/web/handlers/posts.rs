use axum::{
	Json,
	extract::{Path, State},
	response::IntoResponse,
};

use super::common::{ApiResponse, ErrorDto, PostDto, internal_error, not_found};
use crate::web::state::AppState;

/// Get a single post by site and ID.
#[utoipa::path(
	get,
	path = "/api/post/{site}/{post_id}",
	params(
		("site" = String, Path, description = "Site name (e.g. rule34, e621)"),
		("post_id" = u64, Path, description = "Site-local post ID"),
	),
	responses(
		(status = 200, description = "Post found", body = ApiResponse<PostDto>),
		(status = 404, description = "Post not found", body = ApiResponse<ErrorDto>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "posts"
)]
pub async fn get_post(
	Path((site, post_id)): Path<(String, u64)>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	match state.store.fetch_post(&site, post_id).await {
		Ok(Some(r)) => {
			let dto = PostDto::from_result(r, None);
			Json(ApiResponse::ok(dto)).into_response()
		}
		Ok(None) => not_found(format!("Post {site}/{post_id} not found")),
		Err(e) => {
			tracing::error!(error = %e, "post fetch failed");
			internal_error(format!("Post fetch failed: {e}"))
		}
	}
}

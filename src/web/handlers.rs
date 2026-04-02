//! REST API request handlers for the web server.

use axum::{
	Json,
	extract::{Query, State},
	response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::state::AppState;

// ── Search Endpoint ─────────────────────────────────────────────────────────

/// Query parameters for the search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
	/// Text query for semantic search.
	pub q: Option<String>,
	/// Optional site filter (e.g., "rule34").
	pub site: Option<String>,
	/// Maximum number of results to return (default: 10).
	pub limit: Option<u64>,
}

/// Search result returned to the client.
#[derive(Debug, Serialize)]
pub struct SearchResultDto {
	pub post_id: u64,
	pub post_url: String,
	pub thumbnail_url: String,
	pub direct_image_url: String,
	pub width: u32,
	pub height: u32,
	pub score: f32,
}

/// Handle semantic search requests.
///
/// Accepts a text query via the `q` parameter, embeds it using the loaded
/// SigLIP text model, and searches Qdrant for similar images.
pub async fn search(
	Query(params): Query<SearchQuery>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	let limit = params.limit.unwrap_or(10);
	let site_filter = params.site.as_deref();

	let Some(query) = params.q.as_deref().filter(|q| !q.trim().is_empty()) else {
		return (
			axum::http::StatusCode::BAD_REQUEST,
			"Missing or empty query parameter 'q'",
		)
			.into_response();
	};

	let query_vec = match state.embedder.embed_text(query.trim()) {
		Ok(vec) => vec,
		Err(e) => {
			tracing::error!(error = %e, "embedding failed");
			return (
				axum::http::StatusCode::INTERNAL_SERVER_ERROR,
				format!("Embedding failed: {e}"),
			)
				.into_response();
		}
	};

	match state
		.store
		.search(query_vec.to_vec(), limit, site_filter)
		.await
	{
		Ok(results) => {
			let dtos: Vec<SearchResultDto> = results
				.into_iter()
				.map(|r| SearchResultDto {
					post_id: r.post_id,
					post_url: r.post_url,
					thumbnail_url: r.thumbnail_url,
					direct_image_url: r.direct_image_url,
					width: r.width,
					height: r.height,
					score: r.score,
				})
				.collect();
			Json(dtos).into_response()
		}
		Err(e) => {
			tracing::error!(error = %e, "search failed");
			(
				axum::http::StatusCode::INTERNAL_SERVER_ERROR,
				format!("Search failed: {e}"),
			)
				.into_response()
		}
	}
}

// ── Stats Endpoint ──────────────────────────────────────────────────────────

/// Site statistics returned to the client.
#[derive(Debug, Serialize)]
pub struct SiteStatsDto {
	pub total_points: u64,
	pub per_site: Vec<(String, u64)>,
	pub missing_site_payload: u64,
}

/// Handle site statistics requests.
///
/// Returns the total number of indexed points and a per-site breakdown.
pub async fn site_stats(State(state): State<AppState>) -> impl IntoResponse {
	match state.store.fetch_site_counts(1024).await {
		Ok(dist) => Json(SiteStatsDto {
			total_points: dist.total_points,
			per_site: dist.per_site.into_iter().collect(),
			missing_site_payload: dist.missing_site_payload,
		})
		.into_response(),
		Err(e) => {
			tracing::error!(error = %e, "stats fetch failed");
			(
				axum::http::StatusCode::INTERNAL_SERVER_ERROR,
				format!("Stats fetch failed: {e}"),
			)
				.into_response()
		}
	}
}

// ── Ingest Status Endpoint ──────────────────────────────────────────────────

/// Handle ingest status requests.
pub async fn ingest_status(State(state): State<AppState>) -> impl IntoResponse {
	let status = state.ingest_status.read().await;
	Json(status.clone()).into_response()
}

// ── Checkpoint Status Endpoint ──────────────────────────────────────────────

/// Handle checkpoint status requests.
pub async fn checkpoint_status(State(state): State<AppState>) -> impl IntoResponse {
	let status = state.ingest_status.read().await;
	Json(status.last_checkpoint.clone()).into_response()
}

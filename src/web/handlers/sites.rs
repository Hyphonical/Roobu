use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;
use utoipa::ToSchema;

use super::common::{ApiResponse, ErrorDto, internal_error};
use crate::web::state::AppState;

/// Metadata about an indexed site.
#[derive(Debug, Serialize, ToSchema)]
pub struct SiteDto {
	pub name: String,
	pub namespace: u64,
	pub count: u64,
	pub earliest_ingestion: i64,
	pub latest_ingestion: i64,
}

/// List all indexed sites with metadata.
#[utoipa::path(
	get,
	path = "/api/sites",
	responses(
		(status = 200, description = "List of indexed sites", body = ApiResponse<Vec<SiteDto>>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "sites"
)]
pub async fn sites(State(state): State<AppState>) -> impl IntoResponse {
	match state.store.fetch_sites().await {
		Ok(site_infos) => {
			let dtos: Vec<SiteDto> = site_infos
				.into_iter()
				.map(|s| SiteDto {
					name: s.name,
					namespace: s.namespace,
					count: s.count,
					earliest_ingestion: s.earliest_ingestion,
					latest_ingestion: s.latest_ingestion,
				})
				.collect();
			let count = dtos.len();
			Json(ApiResponse::list(dtos, count, None)).into_response()
		}
		Err(e) => {
			tracing::error!(error = %e, "sites fetch failed");
			internal_error(format!("Sites fetch failed: {e}"))
		}
	}
}

use axum::{Json, extract::State, response::IntoResponse};

use super::common::ApiResponse;
use crate::web::state::AppState;

use serde::Serialize;
use utoipa::ToSchema;

/// Snapshot of the ingest service status.
#[derive(Debug, Serialize, ToSchema)]
pub struct IngestStatusDto {
	pub is_running: bool,
	pub active_sites: Vec<String>,
	pub last_checkpoint: Option<serde_json::Value>,
}

impl From<crate::web::state::IngestStatus> for IngestStatusDto {
	fn from(status: crate::web::state::IngestStatus) -> Self {
		Self {
			is_running: status.is_running,
			active_sites: status.active_sites,
			last_checkpoint: status.last_checkpoint,
		}
	}
}

/// Get current ingest status.
#[utoipa::path(
	get,
	path = "/api/ingest/status",
	responses(
		(status = 200, description = "Ingest status snapshot", body = ApiResponse<IngestStatusDto>),
	),
	tag = "ingest"
)]
pub async fn ingest_status(State(state): State<AppState>) -> impl IntoResponse {
	let status = state.current_ingest_status().await;
	Json(ApiResponse::ok(IngestStatusDto::from(status))).into_response()
}

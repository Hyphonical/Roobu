use axum::{
	Json,
	extract::{Query, State},
	response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::common::{ApiResponse, ErrorDto, deserialize_sites, internal_error, parse_sites};
use crate::web::state::AppState;

/// Query parameters for the `/api/activity` endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ActivityParams {
	/// Number of days to look back (default 90, max 365).
	#[schema(example = 90, minimum = 1, maximum = 365)]
	pub days: Option<u64>,

	/// Sites to restrict results to. Repeated param or comma-separated.
	#[schema(example = json!(["rule34", "e621"]))]
	#[serde(default, deserialize_with = "deserialize_sites")]
	pub site: Vec<String>,
}

/// A single day's activity entry.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityDayDto {
	/// Date in ISO 8601 format (YYYY-MM-DD).
	pub date: String,
	/// Number of posts ingested on this day.
	pub count: u64,
	/// Normalized activity level in [0.0, 1.0].
	pub intensity: f64,
}

/// Activity response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityDto {
	pub days: Vec<ActivityDayDto>,
	pub total: u64,
	pub average: f64,
	pub peak: u64,
}

/// Get ingestion activity over time (GitHub-style contribution graph).
#[utoipa::path(
	get,
	path = "/api/activity",
	params(
		("days" = Option<u64>, Query, description = "Days to look back (default 90, max 365)"),
		("site" = Option<Vec<String>>, Query, description = "Sites to filter by (repeated or comma-separated)"),
	),
	responses(
		(status = 200, description = "Activity data", body = ApiResponse<ActivityDto>),
		(status = 500, description = "Internal server error", body = ApiResponse<ErrorDto>),
	),
	tag = "activity"
)]
pub async fn activity(
	Query(params): Query<ActivityParams>,
	State(state): State<AppState>,
) -> impl IntoResponse {
	let days = params.days.unwrap_or(90).clamp(1, 365);
	let sites = parse_sites(&params.site);
	let site_refs: Vec<&str> = sites.iter().map(String::as_str).collect();

	match state.store.fetch_activity(days, &site_refs).await {
		Ok(data) => {
			let peak = data.iter().map(|(_, c)| *c).max().unwrap_or(0);
			let total: u64 = data.iter().map(|(_, c)| *c).sum();
			let average = if days > 0 {
				total as f64 / days as f64
			} else {
				0.0
			};

			let days_dto: Vec<ActivityDayDto> = data
				.into_iter()
				.map(|(date, count)| {
					let intensity = if peak > 0 {
						count as f64 / peak as f64
					} else {
						0.0
					};
					ActivityDayDto {
						date,
						count,
						intensity,
					}
				})
				.collect();

			Json(ApiResponse::ok(ActivityDto {
				days: days_dto,
				total,
				average,
				peak,
			}))
			.into_response()
		}
		Err(e) => {
			tracing::error!(error = %e, "activity fetch failed");
			internal_error(format!("Activity fetch failed: {e}"))
		}
	}
}

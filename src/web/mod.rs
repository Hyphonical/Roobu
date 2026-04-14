//! Web API server (Axum) with REST endpoints, WebSocket support, and Swagger UI.
//!
//! Provides HTTP routes for search, statistics, and ingest progress
//! monitoring via WebSocket. The OpenAPI spec is served at `/api/openapi.json`
//! and the interactive Swagger UI is available at `/swagger-ui/`.

mod handlers;
mod state;
mod ws;

pub use state::AppState;

use axum::{
	Router,
	routing::{get, post},
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::embed::Embedder;
use crate::store::Store;

use self::handlers::{
	ActivityDayDto, ActivityDto, ActivityParams, ApiResponse, ErrorDto, PostDto, RecentParams,
	ResponseMeta, SearchParams, SearchUploadForm, SimilarParams, SiteDto, activity, get_post,
	ingest_status, recent, search, search_similar, search_upload, sites,
};
use self::ws::ws_handler;

// ── OpenAPI Specification ───────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
	info(
		title = "Roobu API",
		version = "0.1.0",
		description = "Semantic image search API for booru sites. Supports text queries, image URL queries, and hybrid text+image searches with optional per-site filtering."
	),
	paths(
		handlers::search::search,
		handlers::search::search_upload,
		handlers::search::search_similar,
		handlers::recent::recent,
		handlers::posts::get_post,
		handlers::activity::activity,
		handlers::sites::sites,
		handlers::ingest::ingest_status,
	),
	components(
		schemas(
			ApiResponse<Vec<PostDto>>,
			ApiResponse<PostDto>,
			ApiResponse<ActivityDto>,
			ApiResponse<Vec<SiteDto>>,
			ApiResponse<handlers::IngestStatusDto>,
			ApiResponse<ErrorDto>,
			ResponseMeta,
			SearchParams,
			SearchUploadForm,
			PostDto,
			SimilarParams,
			RecentParams,
			ActivityParams,
			ActivityDayDto,
			ActivityDto,
			SiteDto,
			handlers::IngestStatusDto,
			ErrorDto,
		)
	),
	tags(
		(name = "search",   description = "Semantic image search"),
		(name = "recent",   description = "Recently ingested posts"),
		(name = "posts",    description = "Individual post lookup"),
		(name = "activity", description = "Ingestion activity timeline"),
		(name = "sites",    description = "Indexed site metadata"),
		(name = "ingest",   description = "Ingest process monitoring"),
	)
)]
struct ApiDoc;

// ── Router ──────────────────────────────────────────────────────────────────

/// Build the Axum router with all API routes, Swagger UI, and middleware.
pub fn create_state(store: Store, embedder: std::sync::Arc<Embedder>) -> AppState {
	AppState::new(store, embedder)
}

/// Build the OpenAPI document for the current API surface.
pub fn openapi_document() -> utoipa::openapi::OpenApi {
	ApiDoc::openapi()
}

/// Build the Axum router for a preconfigured shared application state.
pub fn create_router(state: AppState) -> Router {
	let openapi = openapi_document();

	// Build the stateful API router.
	let api_router = Router::new()
		// ── Public REST API ──────────────────────────────────────────────
		.route("/api/search", get(search))
		.route("/api/search/upload", post(search_upload))
		.route("/api/search/similar/{site}/{post_id}", get(search_similar))
		.route("/api/recent", get(recent))
		.route("/api/post/{site}/{post_id}", get(get_post))
		.route("/api/activity", get(activity))
		.route("/api/sites", get(sites))
		// ── Internal / monitoring ────────────────────────────────────────
		.route("/api/ingest/status", get(ingest_status))
		.route("/api/ws/ingest", get(ws_handler))
		.with_state(state);

	// Swagger UI is stateless — merge it into a plain Router<()> first,
	// then nest the stateful API router alongside it.
	Router::new()
		.merge(SwaggerUi::new("/swagger-ui").url("/api/openapi.json", openapi))
		.merge(api_router)
		.layer(CorsLayer::permissive())
		.layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
	#[test]
	fn openapi_contract_matches_snapshot() {
		let snapshot_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("docs")
			.join("api")
			.join("openapi.v1.json");

		let snapshot_json = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|error| {
			panic!(
				"failed to read OpenAPI snapshot {}: {error}",
				snapshot_path.display()
			)
		});

		let snapshot_value = normalize_json(
			serde_json::from_str::<serde_json::Value>(&snapshot_json).unwrap_or_else(|error| {
				panic!(
					"invalid JSON in OpenAPI snapshot {}: {error}",
					snapshot_path.display()
				)
			}),
		);
		let generated_value = normalize_json(
			serde_json::to_value(super::openapi_document())
				.expect("failed to serialize generated OpenAPI document"),
		);

		assert_eq!(
			snapshot_value,
			generated_value,
			"OpenAPI contract drift detected. Run `roobu contract export --output {}`.",
			snapshot_path.display()
		);
	}

	fn normalize_json(value: serde_json::Value) -> serde_json::Value {
		match value {
			serde_json::Value::Object(map) => {
				let mut keys: Vec<String> = map.keys().cloned().collect();
				keys.sort();
				let mut normalized = serde_json::Map::new();
				for key in keys {
					if let Some(child) = map.get(&key) {
						normalized.insert(key, normalize_json(child.clone()));
					}
				}
				serde_json::Value::Object(normalized)
			}
			serde_json::Value::Array(items) => {
				serde_json::Value::Array(items.into_iter().map(normalize_json).collect())
			}
			other => other,
		}
	}
}

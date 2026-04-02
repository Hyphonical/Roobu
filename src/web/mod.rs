//! Web API server (Axum) with REST endpoints and WebSocket support.
//!
//! Provides HTTP routes for search, statistics, and ingest progress
//! monitoring via WebSocket.

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

use crate::embed::Embedder;
use crate::store::Store;

use self::handlers::{checkpoint_status, ingest_status, search, site_stats};
use self::ws::ws_handler;

/// Build the Axum router with all API routes and middleware.
pub fn create_router(store: Store, embedder: std::sync::Arc<Embedder>) -> Router {
	let state = AppState::new(store, embedder);

	Router::new()
		.route("/api/search", post(search))
		.route("/api/stats", get(site_stats))
		.route("/api/ingest/status", get(ingest_status))
		.route("/api/checkpoint", get(checkpoint_status))
		.route("/api/ws/ingest", get(ws_handler))
		.layer(CorsLayer::permissive())
		.layer(TraceLayer::new_for_http())
		.with_state(state)
}

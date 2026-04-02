//! Shared application state for web handlers.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::embed::Embedder;
use crate::store::Store;

/// Shared application state, cloned into each request handler.
#[derive(Clone)]
pub struct AppState {
	pub store: Arc<Store>,
	pub embedder: Arc<Embedder>,
	pub ingest_status: Arc<RwLock<IngestStatus>>,
}

/// Current ingest status exposed via the API.
#[derive(Clone, Debug, serde::Serialize)]
pub struct IngestStatus {
	pub is_running: bool,
	pub active_sites: Vec<String>,
	pub last_checkpoint: Option<serde_json::Value>,
}

impl AppState {
	/// Create new application state with the given store and embedder.
	pub fn new(store: Store, embedder: Arc<Embedder>) -> Self {
		Self {
			store: Arc::new(store),
			embedder,
			ingest_status: Arc::new(RwLock::new(IngestStatus {
				is_running: false,
				active_sites: Vec::new(),
				last_checkpoint: None,
			})),
		}
	}
}

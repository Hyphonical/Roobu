//! Shared application state for web handlers.

use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use crate::embed::Embedder;
use crate::ingest::events::IngestEvent;
use crate::store::Store;

/// Shared application state, cloned into each request handler.
#[derive(Clone)]
pub struct AppState {
	pub store: Arc<Store>,
	pub embedder: Arc<Embedder>,
	pub ingest_status: Arc<RwLock<IngestStatus>>,
	ingest_events_tx: broadcast::Sender<IngestEvent>,
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
		let (ingest_events_tx, _) = broadcast::channel(256);

		Self {
			store: Arc::new(store),
			embedder,
			ingest_status: Arc::new(RwLock::new(IngestStatus {
				is_running: false,
				active_sites: Vec::new(),
				last_checkpoint: None,
			})),
			ingest_events_tx,
		}
	}

	/// Subscribe to ingest progress events.
	pub fn subscribe_ingest_events(&self) -> broadcast::Receiver<IngestEvent> {
		self.ingest_events_tx.subscribe()
	}

	/// Read the current ingest status snapshot.
	pub async fn current_ingest_status(&self) -> IngestStatus {
		self.ingest_status.read().await.clone()
	}

	/// Publish an ingest event, update status, and broadcast to websocket subscribers.
	pub async fn publish_ingest_event(&self, event: IngestEvent) {
		{
			let mut status = self.ingest_status.write().await;
			update_status_from_event(&mut status, &event);
		}

		if let Err(error) = self.ingest_events_tx.send(event) {
			tracing::debug!(error = %error, "no ingest websocket subscribers");
		}
	}
}

fn update_status_from_event(status: &mut IngestStatus, event: &IngestEvent) {
	match event {
		IngestEvent::StatusSnapshot {
			is_running,
			active_sites,
			last_checkpoint,
		} => {
			status.is_running = *is_running;
			status.active_sites = active_sites.clone();
			status.last_checkpoint = last_checkpoint.clone();
		}
		IngestEvent::CycleComplete { site, .. }
		| IngestEvent::CycleFailed { site, .. }
		| IngestEvent::Sleeping { site, .. } => {
			status.is_running = true;
			mark_site_active(status, site);
		}
		IngestEvent::CheckpointUpdated { site, last_id } => {
			status.is_running = true;
			mark_site_active(status, site);
			update_checkpoint_site(status, site, *last_id);
		}
	}
}

fn mark_site_active(status: &mut IngestStatus, site: &str) {
	if status.active_sites.iter().any(|active| active == site) {
		return;
	}

	status.active_sites.push(site.to_string());
	status.active_sites.sort();
}

fn update_checkpoint_site(status: &mut IngestStatus, site: &str, last_id: u64) {
	let mut checkpoint_map = status
		.last_checkpoint
		.clone()
		.and_then(|value| value.as_object().cloned())
		.unwrap_or_default();

	checkpoint_map.insert(site.to_string(), serde_json::json!(last_id));
	status.last_checkpoint = Some(serde_json::Value::Object(checkpoint_map));
}

use std::sync::Arc;

use serde::Serialize;

/// Structured ingest events used by websocket streaming and status monitoring.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum IngestEvent {
	/// Full status snapshot for clients that just connected.
	StatusSnapshot {
		is_running: bool,
		active_sites: Vec<String>,
		last_checkpoint: Option<serde_json::Value>,
	},
	/// A site cycle completed successfully.
	CycleComplete {
		site: String,
		fetched: usize,
		upserted: usize,
		elapsed_secs: f64,
	},
	/// A site cycle failed.
	CycleFailed {
		site: String,
		error: String,
		elapsed_secs: f64,
	},
	/// Checkpoint was updated and persisted.
	CheckpointUpdated { site: String, last_id: u64 },
	/// Ingest loop is sleeping between cycles.
	Sleeping { site: String, sleep_secs: u64 },
}

/// Shared callback type for publishing ingest events.
pub type IngestEventSink = Arc<dyn Fn(IngestEvent) + Send + Sync>;

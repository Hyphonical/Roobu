//! WebSocket handler for real-time ingest progress streaming.

use axum::{
	extract::State,
	extract::ws::{Message, WebSocket, WebSocketUpgrade},
	response::IntoResponse,
};
use serde::Serialize;

use super::state::AppState;

/// Events sent over the WebSocket ingest progress channel.
///
/// These are broadcast to connected clients as ingest cycles run,
/// enabling real-time progress monitoring in the frontend.
#[allow(dead_code)]
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum IngestProgressEvent {
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

/// Upgrade an HTTP request to a WebSocket connection.
pub async fn ws_handler(ws: WebSocketUpgrade, State(_state): State<AppState>) -> impl IntoResponse {
	ws.on_upgrade(handle_socket)
}

/// Handle an established WebSocket connection.
///
/// Sends a welcome message and keeps the connection alive by responding
/// to pings. In a full implementation, this would subscribe to a broadcast
/// channel that receives events from the ingest loop.
async fn handle_socket(mut socket: WebSocket) {
	// Send a welcome message to confirm the connection.
	if socket
		.send(Message::Text(
			serde_json::json!({
				"type": "connected",
				"data": { "message": "Connected to Roobu ingest progress channel" }
			})
			.to_string()
			.into(),
		))
		.await
		.is_err()
	{
		return;
	}

	// Keep the connection alive and listen for client messages.
	while let Some(msg) = socket.recv().await {
		match msg {
			Ok(Message::Text(text)) => {
				tracing::debug!(%text, "ws client message");
			}
			Ok(Message::Close(_)) => {
				tracing::debug!("ws client disconnected");
				break;
			}
			Ok(Message::Ping(data)) => {
				let _ = socket.send(Message::Pong(data)).await;
			}
			Err(e) => {
				tracing::warn!(error = %e, "ws error");
				break;
			}
			_ => {}
		}
	}
}

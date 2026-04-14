//! WebSocket handler for real-time ingest progress streaming.

use std::time::Duration;

use axum::{
	extract::State,
	extract::ws::{Message, WebSocket, WebSocketUpgrade},
	response::IntoResponse,
};
use tokio::sync::broadcast::error::RecvError;

use super::state::AppState;
use crate::ingest::events::IngestEvent;

/// Upgrade an HTTP request to a WebSocket connection.
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
	ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an established WebSocket connection.
///
/// Sends an initial snapshot and then streams ingest events through a
/// broadcast subscription while handling control frames from the client.
async fn handle_socket(mut socket: WebSocket, state: AppState) {
	let mut event_rx = state.subscribe_ingest_events();

	// Send a welcome message to confirm the connection.
	if send_json_value(
		&mut socket,
		serde_json::json!({
			"type": "connected",
			"data": { "message": "Connected to Roobu ingest progress channel" }
		}),
	)
	.await
	.is_err()
	{
		return;
	}

	let snapshot = state.current_ingest_status().await;
	if send_ingest_event(
		&mut socket,
		IngestEvent::StatusSnapshot {
			is_running: snapshot.is_running,
			active_sites: snapshot.active_sites,
			last_checkpoint: snapshot.last_checkpoint,
		},
	)
	.await
	.is_err()
	{
		return;
	}

	let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
	heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	loop {
		tokio::select! {
			incoming = socket.recv() => {
				match incoming {
					Some(Ok(Message::Text(text))) => {
						tracing::debug!(%text, "ws client message");
					}
					Some(Ok(Message::Ping(data))) => {
						if socket.send(Message::Pong(data)).await.is_err() {
							break;
						}
					}
					Some(Ok(Message::Close(_))) => {
						tracing::debug!("ws client disconnected");
						break;
					}
					Some(Err(error)) => {
						tracing::warn!(error = %error, "ws read error");
						break;
					}
					None => break,
					_ => {}
				}
			}
			event = event_rx.recv() => {
				match event {
					Ok(event) => {
						if send_ingest_event(&mut socket, event).await.is_err() {
							break;
						}
					}
					Err(RecvError::Lagged(dropped)) => {
						tracing::warn!(dropped, "ws ingest stream lagged");
						if send_json_value(
							&mut socket,
							serde_json::json!({
								"type": "lagged",
								"data": { "dropped": dropped }
							}),
						)
						.await
						.is_err()
						{
							break;
						}
					}
					Err(RecvError::Closed) => {
						tracing::debug!("ws ingest event source closed");
						break;
					}
				}
			}
			_ = heartbeat.tick() => {
				if socket.send(Message::Ping(Vec::<u8>::new().into())).await.is_err() {
					break;
				}
			}
		}
	}
}

async fn send_ingest_event(socket: &mut WebSocket, event: IngestEvent) -> Result<(), ()> {
	let serialized = serde_json::to_string(&event).map_err(|error| {
		tracing::warn!(error = %error, "failed to serialize ingest event");
	})?;

	socket
		.send(Message::Text(serialized.into()))
		.await
		.map_err(|_| ())
}

async fn send_json_value(socket: &mut WebSocket, value: serde_json::Value) -> Result<(), ()> {
	socket
		.send(Message::Text(value.to_string().into()))
		.await
		.map_err(|_| ())
}

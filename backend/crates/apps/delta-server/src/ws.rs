//! Browser event WebSocket.
//!
//! Each connected browser subscribes to the process-wide event stream and
//! receives JSON-encoded [`SessionEvent`]s: session registered, turn started,
//! external input, turn completed, and permission requested.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;

pub async fn ws_handler(
    State(state): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| pump_events(socket, state))
}

/// Forward broadcast events to one browser socket until it closes.
async fn pump_events(mut socket: WebSocket, state: AppState) {
    let mut rx = state.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                let payload = match serde_json::to_string(&event) {
                    Ok(json) => json,
                    Err(err) => {
                        tracing::error!(error = %err, "failed to encode session event");
                        continue;
                    }
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break; // client disconnected
                }
            }
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "browser event stream lagged");
            }
            Err(RecvError::Closed) => break,
        }
    }
}

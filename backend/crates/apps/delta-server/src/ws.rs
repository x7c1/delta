//! Browser event WebSocket.
//!
//! Each connected browser subscribes to the process-wide event stream and
//! receives every session event as JSON — whatever
//! [`SessionEvent`](delta_usecase::SessionEvent) currently declares, with
//! each variant's payload documented for clients in
//! `docs/guides/api/live-channels.md`. Every event is id-routed by
//! `session_id`; focus is purely client-side, so there is no server-side
//! focus event.
//!
//! Domain [`SessionEvent`](delta_usecase::SessionEvent)s are converted to
//! their wire twin [`WireSessionEvent`] at this boundary; the wire crate owns
//! the JSON shape and the TypeScript bindings generated from it.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use delta_wire::WireSessionEvent;
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;

pub async fn ws_handler(
    State(state): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| pump_events(socket, state))
}

/// Forward broadcast events to one browser socket until it closes.
///
/// The receive half is polled alongside the broadcast channel so a client
/// close is honored immediately: consuming the close frame lets the close
/// handshake complete (otherwise the browser socket hangs in `CLOSING`), and
/// the task — with its broadcast subscription — ends right away instead of
/// leaking until the next send to the dead socket fails. The stream carries no
/// client-to-server messages, so anything else received is ignored.
async fn pump_events(mut socket: WebSocket, state: AppState) {
    let mut rx = state.subscribe();
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(event) => {
                    let payload = match serde_json::to_string(&WireSessionEvent::from(event)) {
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
            },
            incoming = socket.recv() => match incoming {
                // A close frame or a dropped connection: this subscriber is
                // done. Pings are answered by axum itself; any other frame is
                // unexpected on this one-way stream and simply dropped.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

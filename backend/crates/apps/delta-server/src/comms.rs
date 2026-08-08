//! The comms-log stream: `/comms?session_id=<id>`.
//!
//! A session driven headlessly over a structured transport has no terminal to
//! watch, so this is its window: the JSON-RPC frames Delta exchanged with the
//! provider, time-ordered, streamed to the browser as JSON text frames (one
//! [`WireCommsFrame`] each).
//!
//! Deliberately its own route rather than a variant on `/ws`:
//!
//! - `/ws` is the process-wide conversation stream every browser tab already
//!   holds open, and the frames here are per-session, high-volume, and
//!   interesting to at most the one pane looking at them. Putting them on `/ws`
//!   would push them at every tab whether or not any pane was open;
//! - `/ws` carries facts Delta *acted on*; this carries what went over the wire.
//!   Keeping them separate is what lets the log be lossy and unpersisted (see
//!   [`crate::comms_log`]) without weakening the conversation stream's contract.
//!
//! A socket for a session with no live wire — closed, dormant, or Claude-backed
//! (which has a terminal instead) — is not an error: it opens, replays whatever
//! is buffered (usually nothing), and stays quiet. The pane shows its idle state
//! rather than a failure, since "nothing is being exchanged" is the honest
//! answer.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::comms_log::CommsSubscription;
use crate::state::AppState;

/// Query parameters for the comms-log stream: whose log to watch.
#[derive(Debug, Deserialize)]
pub struct CommsQuery {
    /// Delta's own session id — the id the browser already has. The log is keyed
    /// by it (never by the provider's thread id), so no lookup is needed here.
    session_id: String,
}

pub async fn comms_handler(
    State(state): State<AppState>,
    Query(query): Query<CommsQuery>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    // Subscribe BEFORE the upgrade completes, so frames recorded during the
    // handshake are already on this watcher's stream rather than lost between
    // accepting the socket and joining the log.
    let watcher = state.watch_comms_log(&query.session_id);
    upgrade.on_upgrade(move |socket| pump_frames(socket, watcher))
}

/// Forward one session's frames to one browser socket until either side ends.
///
/// The receive half is polled alongside the log so a client close is honored
/// immediately (mirroring `/ws`): consuming the close frame lets the handshake
/// complete instead of leaving the browser socket in `CLOSING`, and this task —
/// with its subscription — ends at once rather than lingering until the next
/// send fails. The stream carries no client-to-server messages, so anything else
/// received is ignored.
async fn pump_frames(mut socket: WebSocket, mut watcher: CommsSubscription) {
    loop {
        tokio::select! {
            frame = watcher.next() => match frame {
                Some(frame) => {
                    let payload = match serde_json::to_string(&frame) {
                        Ok(json) => json,
                        Err(err) => {
                            tracing::error!(error = %err, "failed to encode comms frame");
                            continue;
                        }
                    };
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        break; // client disconnected
                    }
                }
                // The session's log is gone (the session ended): nothing further
                // will ever arrive on this stream, so close it.
                None => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

//! PTY bridge between the browser terminal and the tmux pane.
//!
//! The browser runs an xterm.js terminal and connects here over a WebSocket,
//! naming the session to attach to via the `session_id` query parameter
//! (`/pty?session_id=<id>`). The server resolves that session's pane from the
//! registry, spawns `tmux attach-session` against it inside a pseudo-terminal,
//! and bridges bytes both ways: PTY output is streamed to the browser as binary
//! frames, and browser frames carry two kinds, distinguished by WebSocket frame
//! type. Binary frames are raw input bytes written into the PTY. Text frames are
//! JSON control messages; the only one today is resize,
//! `{"type":"resize","rows":<u16>,"cols":<u16>}`, which resizes the PTY master
//! so tmux and the pane program follow the browser terminal's dimensions.
//! Malformed or unknown control messages are logged and ignored so they never
//! break the bridge.

use std::io::{Read, Write};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;

use delta_usecase::SessionId;

use crate::state::AppState;

/// Query parameters for the PTY bridge: the session to attach to.
#[derive(Debug, Deserialize)]
pub struct PtyQuery {
    session_id: String,
}

/// A browser → server control message, carried on a WebSocket Text frame. Input
/// bytes travel on Binary frames; Text frames are reserved for control. The only
/// variant today is `resize`, which adjusts the PTY master's dimensions so tmux
/// and the pane program track the browser terminal.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ControlMessage {
    Resize { rows: u16, cols: u16 },
}

pub async fn pty_handler(
    State(state): State<AppState>,
    Query(query): Query<PtyQuery>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    // Resolve the named session's pane up front. If it is not open there is
    // nothing to attach to, so the bridge closes the socket cleanly instead of
    // attaching to a non-existent pane.
    let session_id = SessionId::from(query.session_id);
    let pane = state.pane_for_session(&session_id).await;
    let tmux_socket = state.tmux_socket().to_owned();

    // Clear any residual input before the fresh attach. When the previous PTY
    // bridge tore down (e.g. a browser reload detached the attach client), tmux
    // delivered a focus-out report (`ESC[O`) to the pane program, which Claude
    // renders as a stray blank line in its input box. Wiping it here means a
    // reconnect shows a clean input. This only fires on a real (re)attach, so a
    // normal hidden->shown session switch — which keeps its persistent
    // connection and does not reconnect — is unaffected. Clear-on-send remains
    // the guarantee for message integrity; this is the complementary
    // clear-on-attach. A failed clear must never block the attach, so it is
    // logged and ignored rather than propagated.
    if pane.is_some() {
        if let Err(err) = state.clear_session_input(&session_id).await {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "failed to clear pane input before attach; continuing"
            );
        }
    }

    upgrade.on_upgrade(move |socket| bridge(socket, session_id, pane, tmux_socket))
}

/// Bridge a browser WebSocket to a tmux pane through a PTY.
///
/// When the named session is not open (`pane` is `None`) there is nothing to
/// attach to: log it and let the socket close.
async fn bridge(
    mut socket: WebSocket,
    session_id: SessionId,
    pane: Option<String>,
    tmux_socket: String,
) {
    let Some(pane) = pane else {
        tracing::warn!(
            session_id = %session_id,
            "pty bridge requested for a session that is not open; closing"
        );
        let _ = socket.close().await;
        return;
    };
    if let Err(err) = run_bridge(socket, pane, &tmux_socket).await {
        tracing::error!(error = %err, "pty bridge terminated with error");
    }
}

async fn run_bridge(socket: WebSocket, pane: String, tmux_socket: &str) -> anyhow::Result<()> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Attach to the existing tmux session/pane in read-write mode, on Delta's
    // dedicated tmux server (`-L <socket>`) — the same server the sessions are
    // created on.
    let mut cmd = CommandBuilder::new("tmux");
    cmd.arg("-L");
    cmd.arg(tmux_socket);
    cmd.arg("attach-session");
    cmd.arg("-t");
    cmd.arg(&pane);
    let mut child = pair.slave.spawn_command(cmd)?;

    // The blocking PTY reader/writer halves run on dedicated threads; bytes are
    // shuttled to/from the async socket via channels. The master itself is moved
    // into the async input task below so it can service resize control messages;
    // it is otherwise idle once the reader and writer are taken.
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let master = pair.master;

    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();

    // PTY -> channel (blocking read loop).
    let read_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // channel -> PTY (blocking write loop).
    let write_thread = std::thread::spawn(move || {
        while let Ok(bytes) = in_rx.recv() {
            if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                break;
            }
        }
    });

    let (mut sink, mut stream) = socket.split();

    // Socket -> PTY input and control. Binary frames are raw input bytes;
    // Text frames are JSON control messages (resize). The master is owned here
    // so a resize can be applied directly.
    let mut input_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(b) => {
                    if in_tx.send(b.into()).is_err() {
                        break;
                    }
                }
                Message::Text(t) => match serde_json::from_str::<ControlMessage>(&t) {
                    Ok(ControlMessage::Resize { rows, cols }) => {
                        if let Err(err) = master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        }) {
                            tracing::warn!(error = %err, "failed to resize pty");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "ignoring malformed pty control message");
                    }
                },
                Message::Close(_) => break,
                _ => continue,
            }
        }
    });

    // Pump PTY output to the socket until EITHER side ends: the output drains
    // (or the sink fails), OR the input task finishes because the browser closed
    // the socket. Gating teardown on the output loop alone leaks the bridge — a
    // reload with no further pane output never wakes `out_rx.recv()` (the read
    // thread stays blocked on the PTY until the child is killed, which only
    // happens in teardown), and `sink.send` is never called either, so the
    // child, the reader/writer threads, and the socket would linger forever. One
    // leaked bridge per reload eventually starves the runtime. Selecting on the
    // input task's completion tears the bridge down promptly on socket close.
    loop {
        tokio::select! {
            chunk = out_rx.recv() => match chunk {
                Some(chunk) => {
                    if sink.send(Message::Binary(chunk.into())).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            _ = &mut input_task => break,
        }
    }

    // Tear down. Kill the child first so the blocking read thread sees EOF and
    // exits; aborting the input task drops the input sender so the write thread's
    // channel closes. Both joins then return promptly rather than blocking the
    // runtime, and closing the sink releases the socket instead of leaving it in
    // CLOSE-WAIT.
    input_task.abort();
    let _ = child.kill();
    let _ = sink.close().await;
    let _ = read_thread.join();
    let _ = write_thread.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-formed resize control message deserializes into the `Resize`
    /// variant carrying the requested dimensions.
    #[test]
    fn resize_control_message_deserializes() {
        let msg: ControlMessage =
            serde_json::from_str(r#"{"type":"resize","rows":40,"cols":120}"#)
                .expect("a well-formed resize message deserializes");
        assert!(matches!(
            msg,
            ControlMessage::Resize {
                rows: 40,
                cols: 120,
            }
        ));
    }

    /// An unknown control `type` is rejected rather than panicking, so the input
    /// task can log and ignore it without breaking the bridge.
    #[test]
    fn unknown_control_message_type_is_rejected() {
        let result = serde_json::from_str::<ControlMessage>(r#"{"type":"explode"}"#);
        assert!(result.is_err(), "an unknown control type is not accepted");
    }

    /// Garbage that is not even JSON is rejected the same way.
    #[test]
    fn garbage_control_message_is_rejected() {
        let result = serde_json::from_str::<ControlMessage>("not json at all");
        assert!(result.is_err(), "non-JSON text is not accepted");
    }
}

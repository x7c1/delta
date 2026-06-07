//! PTY bridge between the browser terminal and the tmux pane.
//!
//! The browser runs an xterm.js terminal and connects here over a WebSocket.
//! The server resolves the open session's pane from the registry, spawns
//! `tmux attach-session` against it inside a pseudo-terminal, and bridges bytes
//! both ways: PTY output is streamed to the
//! browser as binary frames, and browser input frames are written back into the
//! PTY. This is a deliberately minimal attach; resize negotiation and richer
//! control messages can be layered on later without changing the route.

use std::io::{Read, Write};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize};
use tokio::sync::mpsc;

use crate::state::AppState;

pub async fn pty_handler(
    State(state): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    // Resolve the open session's pane up front. With no session open there is
    // nothing to attach to, so the bridge closes the socket cleanly instead of
    // attaching to a non-existent pane.
    let pane = state.focused_pane().await;
    upgrade.on_upgrade(move |socket| bridge(socket, pane))
}

/// Bridge a browser WebSocket to a tmux pane through a PTY.
///
/// With no open session (`pane` is `None`) there is nothing to attach to: log it
/// and let the socket close.
async fn bridge(mut socket: WebSocket, pane: Option<String>) {
    let Some(pane) = pane else {
        tracing::warn!("pty bridge requested with no open session; closing");
        let _ = socket.close().await;
        return;
    };
    if let Err(err) = run_bridge(socket, pane).await {
        tracing::error!(error = %err, "pty bridge terminated with error");
    }
}

async fn run_bridge(socket: WebSocket, pane: String) -> anyhow::Result<()> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Attach to the existing tmux session/pane in read-write mode.
    let mut cmd = CommandBuilder::new("tmux");
    cmd.arg("attach-session");
    cmd.arg("-t");
    cmd.arg(&pane);
    let mut child = pair.slave.spawn_command(cmd)?;

    // The blocking PTY reader/writer halves run on dedicated threads; bytes are
    // shuttled to/from the async socket via channels.
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;

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

    // Socket -> PTY input.
    let input_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let bytes: Vec<u8> = match msg {
                Message::Binary(b) => b.into(),
                Message::Text(t) => t.as_bytes().to_vec(),
                Message::Close(_) => break,
                _ => continue,
            };
            if in_tx.send(bytes).is_err() {
                break;
            }
        }
    });

    // PTY output -> socket.
    while let Some(chunk) = out_rx.recv().await {
        if sink.send(Message::Binary(chunk.into())).await.is_err() {
            break;
        }
    }

    // Tear down: dropping the channels unblocks the threads.
    input_task.abort();
    let _ = child.kill();
    let _ = read_thread.join();
    let _ = write_thread.join();
    Ok(())
}

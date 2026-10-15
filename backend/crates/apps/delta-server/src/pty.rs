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
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;

use delta_usecase::SessionId;

use crate::socket_limits::{bound_inbound, SocketSlot, PTY_INBOUND_LIMIT};
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

/// Accept a terminal socket, within the session's socket cap and with inbound
/// messages bounded to a generous paste (see [`crate::socket_limits`]).
pub async fn pty_handler(
    State(state): State<AppState>,
    Query(query): Query<PtyQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let slot = match state.sockets().acquire_pty(&query.session_id) {
        Ok(slot) => slot,
        Err(refused) => return refused.into_response(),
    };
    let session_id = SessionId::from(query.session_id);
    bound_pty_inbound(upgrade).on_upgrade(move |socket| bridge(socket, state, session_id, slot))
}

/// `upgrade` with the terminal bridge's inbound bound applied.
fn bound_pty_inbound(upgrade: WebSocketUpgrade) -> WebSocketUpgrade {
    bound_inbound(upgrade, PTY_INBOUND_LIMIT)
}

/// Bridge a browser WebSocket to a tmux pane through a PTY.
///
/// Resolving the pane happens here, after the upgrade, rather than in the
/// handler: resolving also *records* the attach, and an upgrade that never
/// completes would leave that record behind with no bridge to release it —
/// telling the launch watchdog forever that somebody is watching a pane nobody
/// is. When nothing resolves, the socket is logged and closed.
///
/// `_slot` is this socket's place under the session's connection cap, held for
/// exactly as long as the bridge runs.
async fn bridge(mut socket: WebSocket, state: AppState, session_id: SessionId, _slot: SocketSlot) {
    // A bound session resolves its pane, and so does a fresh spawn whose pane is
    // up but unbound; anything else resolves nothing, and the bridge closes the
    // socket cleanly rather than attach to a pane that does not exist.
    let Some(attached) = state.attach_pane(&session_id).await else {
        tracing::warn!(
            session_id = %session_id,
            "pty bridge requested for a session with no attachable pane; closing"
        );
        let _ = socket.close().await;
        return;
    };
    // The attach is now recorded on the session's actor and must be given back
    // however this function leaves, a panic inside the bridge included.
    let _guard = AttachGuard {
        state: state.clone(),
        session_id: session_id.clone(),
    };

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
    //
    // Only for a BOUND pane. The wipe is `C-u` plus a run of backspaces — keys,
    // not an abstract "empty the input" — and it is safe only because a bound
    // pane is known to be an agent sitting at its prompt. An unbound spawn's
    // pane may be showing anything, up to and including a dialog whose options
    // those very keys would answer, and it has no residual input to wipe anyway:
    // nothing has ever typed into it, and no client has detached from it.
    if attached.bound {
        if let Err(err) = state.clear_session_input(&session_id).await {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "failed to clear pane input before attach; continuing"
            );
        }
    }

    let attach = attach_command(state.tmux_socket(), &attached.pane, state.child_env());
    if let Err(err) = run_bridge(socket, attach).await {
        tracing::error!(error = %err, "pty bridge terminated with error");
    }
}

/// Holds the session's recorded PTY attach for as long as the bridge lives, and
/// releases it on drop.
///
/// Dropping is the one place every exit path passes through, and the release is
/// posted to the actor from a detached task because `Drop` cannot await.
struct AttachGuard {
    state: AppState,
    session_id: SessionId,
}

impl Drop for AttachGuard {
    fn drop(&mut self) {
        let state = self.state.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move { state.detach_pane(&session_id).await });
    }
}

/// The `tmux` arguments that attach to `pane` in read-write mode, on Delta's
/// dedicated tmux server (`-L <socket>`) — the same server the sessions are
/// created on.
fn attach_args<'a>(tmux_socket: &'a str, pane: &'a str) -> [&'a str; 6] {
    [
        // The client's terminal is always xterm.js, which speaks UTF-8; without
        // `-u` a client started with no UTF-8 locale prints `_` for wide chars.
        "-u",
        "-L",
        tmux_socket,
        "attach-session",
        "-t",
        pane,
    ]
}

/// The `tmux attach` client for `pane`, with `env` (the server
/// configuration's `child_env`) set on top of the inherited environment, so
/// `tmux` is found on the same `PATH` and runs under the same locale as the
/// tmux driver's commands.
fn attach_command(tmux_socket: &str, pane: &str, env: &[(String, String)]) -> CommandBuilder {
    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(attach_args(tmux_socket, pane));
    for (name, value) in env {
        cmd.env(name, value);
    }
    // Pin the attach client's TERM instead of inheriting the server's. The
    // terminal on the other side of this PTY is always the browser's xterm.js
    // (an xterm-compatible emulator), so xterm-256color describes it correctly
    // no matter how the server was launched. Inheriting breaks headless
    // launches (CI runners, service managers) where TERM is unset or `dumb`:
    // tmux then refuses the attach ("open terminal failed: terminal does not
    // support clear") and exits at once, leaving the bridge writing keystrokes
    // into a dead client.
    cmd.env("TERM", "xterm-256color");
    cmd
}

async fn run_bridge(socket: WebSocket, attach: CommandBuilder) -> anyhow::Result<()> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut child = pair.slave.spawn_command(attach)?;
    // Close this process's copy of the slave now that the child holds its own.
    // The read thread below only returns once the master reports the pane gone
    // (EOF or EIO), and on Linux that happens when the LAST slave descriptor
    // closes: killing the attach client (the pty's session leader) does not
    // hang the slave up there, unlike macOS, which revokes it. Kept open, the
    // slave would park the read thread forever after teardown kills the child,
    // so the join below would never return, the bridge would never finish, and
    // its `AttachGuard` would never give the attach back — the launch watchdog
    // would then treat the pane as watched for good and never reap it.
    drop(pair.slave);

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
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(msg) => msg,
                // A dropped connection, or an inbound message over its bound.
                Err(err) => {
                    tracing::debug!(error = %err, "pty socket receive failed; closing");
                    break;
                }
            };
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
    let mut input_done = false;
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
            // The browser closed the socket: the input task ran to completion in
            // this arm, so its JoinHandle is now resolved. Record that so the
            // teardown does NOT poll it again.
            _ = &mut input_task => {
                input_done = true;
                break;
            }
        }
    }

    // Tear down WITHOUT blocking the async runtime. The reader/writer halves are
    // blocking OS threads; joining them directly on a worker is what wedges the
    // runtime under repeated reloads. `write_thread` only exits once its input
    // channel closes, and `input_task.abort()` does not synchronously drop the
    // task's `in_tx` — so a bare `write_thread.join()` parks the worker until the
    // task happens to be cleaned up, and enough teardowns starve every worker,
    // freezing the whole server (even unrelated routes stop responding).
    //
    // So when the bridge ended because the PTY output drained or the sink failed
    // (the input task is still running), abort it and AWAIT it so its captured
    // `in_tx` and PTY master are actually dropped (closing the write channel and
    // the master fd). But when the input task already finished above (the browser
    // closed the socket — the common case), its JoinHandle is resolved and its
    // captures are already dropped; awaiting it again panics with "JoinHandle
    // polled after completion" and would skip the cleanup below, leaking the
    // child and threads. Guard on `input_done` so we poll it at most once.
    if !input_done {
        input_task.abort();
        let _ = input_task.await;
    }
    // Kill the child so the read thread sees EOF, close the sink to release the
    // socket, then join both now-finishing threads on a blocking-pool thread
    // rather than a runtime worker.
    let _ = child.kill();
    let _ = sink.close().await;
    let _ = tokio::task::spawn_blocking(move || {
        let _ = read_thread.join();
        let _ = write_thread.join();
    })
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attach client declares a UTF-8 terminal and targets the pane on
    /// Delta's socket.
    #[test]
    fn attach_declares_utf8_on_the_delta_socket() {
        assert_eq!(
            attach_args("delta", "%3"),
            ["-u", "-L", "delta", "attach-session", "-t", "%3"]
        );
    }

    #[test]
    fn the_attach_client_carries_the_given_environment_and_a_pinned_term() {
        let env = vec![
            ("PATH".to_owned(), "/opt/homebrew/bin:/usr/bin".to_owned()),
            ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
            ("TERM".to_owned(), "dumb".to_owned()),
        ];

        let cmd = attach_command("delta", "%3", &env);

        assert_eq!(
            cmd.get_env("PATH"),
            Some("/opt/homebrew/bin:/usr/bin".as_ref())
        );
        assert_eq!(cmd.get_env("LANG"), Some("en_US.UTF-8".as_ref()));
        assert_eq!(cmd.get_env("TERM"), Some("xterm-256color".as_ref()));
        assert_eq!(
            cmd.get_argv(),
            &["tmux", "-u", "-L", "delta", "attach-session", "-t", "%3"]
        );
    }

    /// A well-formed resize control message deserializes into the `Resize`
    /// variant carrying the requested dimensions.
    #[test]
    fn resize_control_message_deserializes() {
        let msg: ControlMessage = serde_json::from_str(r#"{"type":"resize","rows":40,"cols":120}"#)
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

    mod inbound_bound {
        use axum::routing::get;
        use axum::Router;
        use futures_util::StreamExt;
        use tokio_tungstenite::tungstenite::Message as ClientMessage;

        use super::*;
        use crate::socket_limits::test_client::{
            assert_closed, assert_huge_frame_header_is_refused, connect, send_regardless, serve,
            PATIENCE,
        };

        /// Serve the bridge — with the upgrade bounded as `pty_handler` bounds
        /// it — running `program` in place of the tmux attach client, so the
        /// bound is exercised against a real PTY without a tmux pane.
        async fn serve_bridge(program: &'static [&'static str]) -> u16 {
            let app = Router::new().route(
                "/pty",
                get(move |upgrade: WebSocketUpgrade| async move {
                    bound_pty_inbound(upgrade).on_upgrade(move |socket| async move {
                        let mut command = CommandBuilder::new(program[0]);
                        command.args(&program[1..]);
                        run_bridge(socket, command).await.unwrap();
                    })
                }),
            );
            serve(app).await
        }

        /// A paste of exactly the bound reaches the program behind the PTY
        /// whole: `wc -c` counts every byte of it.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_paste_at_the_bound_is_delivered() {
            let port = serve_bridge(&["wc", "-c"]).await;
            let mut client = connect(port, "/pty").await.unwrap();

            // Newline-terminated lines, so the terminal's line discipline (in
            // its default canonical mode) hands each one to `wc` in turn.
            let line = format!("{}\n", "a".repeat(63));
            let paste = line.repeat(PTY_INBOUND_LIMIT / line.len());
            assert_eq!(paste.len(), PTY_INBOUND_LIMIT);
            send_regardless(&mut client, ClientMessage::binary(paste.into_bytes())).await;
            // End of input, at the start of a line.
            send_regardless(&mut client, ClientMessage::binary(vec![0x04])).await;

            let expected = PTY_INBOUND_LIMIT.to_string();
            let mut output = Vec::new();
            loop {
                match tokio::time::timeout(PATIENCE, client.next()).await {
                    Ok(Some(Ok(ClientMessage::Binary(chunk)))) => {
                        output.extend_from_slice(&chunk);
                        // The count is printed last; only the tail can hold it.
                        let tail = &output[output.len().saturating_sub(64)..];
                        if String::from_utf8_lossy(tail).contains(&expected) {
                            return;
                        }
                    }
                    Ok(Some(Ok(_))) => {}
                    other => {
                        panic!("the bridge ended before `wc` reported {expected} bytes: {other:?}")
                    }
                }
            }
        }

        /// One byte over the bound ends the socket, and so does a frame that
        /// merely announces a huge payload.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_message_over_the_bound_ends_the_bridge() {
            let port = serve_bridge(&["cat"]).await;
            let mut client = connect(port, "/pty").await.unwrap();
            send_regardless(
                &mut client,
                ClientMessage::binary(vec![b'a'; PTY_INBOUND_LIMIT + 1]),
            )
            .await;
            assert_closed(&mut client).await;

            assert_huge_frame_header_is_refused(port, "/pty").await;
        }
    }
}

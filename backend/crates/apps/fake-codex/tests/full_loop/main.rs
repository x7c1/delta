//! The Codex full-loop end-to-end test: browser → server → `fake-codex`.
//!
//! This is the payoff proof for the live Codex event pump. It assembles the
//! **real** backend — `delta_server::router` over an `AppState` wired with the
//! real gateways (an in-memory `SqliteStore`, the real transcript/tmux/workspace
//! gateways, all inert on the terminal-less Codex path) plus a real
//! `CodexAdapterFactory` pointing at the compiled `fake-codex` binary — and
//! drives one turn through the whole stack:
//!
//! 1. `POST /api/sends` with `provider: "codex"` and a first prompt. Composition
//!    dispatches this to the terminal-less Codex path, which stands up the
//!    `fake-codex` subprocess (spawn + `initialize` handshake), starts a thread
//!    (`thread/start`), and starts the turn (`turn/start`).
//! 2. The fake plays a scripted turn: a streaming assistant fragment
//!    (`item/started` with text), the completed assistant message
//!    (`item/completed`), then `turn/completed`.
//! 3. The event pump drains the adapter's event stream and drives the session
//!    actor, which persists the completed message and emits the browser events on
//!    the async seam — reaching the broadcast the WebSocket forwards.
//!
//! That loop is `streaming`'s test, which asserts, over that broadcast, that the
//! assistant message was **streamed** (`AssistantStreaming`) and its turn
//! **completed** (`TurnCompleted`), and, over `GET /api/threads/{id}/messages`,
//! that the assistant message was **persisted** — the create → prompt →
//! assistant → turn-complete loop, offline and deterministic.
//!
//! The sibling loops live beside it, one module per behaviour: launch
//! options, message metadata, the second message, branching from selected text,
//! resume across a restart, the interrupt, permissions, the app-server's death,
//! the comms-log stream, and usage. `support` holds the backend assembly and the
//! request/drain helpers they share.

mod support;

mod branching;
mod comms_log_stream;
mod interrupt;
mod launch_options;
mod message_metadata;
mod permissions;
mod restart;
mod second_message;
mod session_death;
mod streaming;
mod usage;

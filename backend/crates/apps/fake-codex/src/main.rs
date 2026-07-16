//! `fake-codex`: a scripted stand-in for `codex app-server`.
//!
//! Delta drives Codex through a single long-lived `codex app-server` process,
//! observed exclusively through **newline-delimited JSON-RPC 2.0 over stdio**:
//! the client writes request/notification frames to the server's stdin and
//! reads response/notification/server-request frames from its stdout. This
//! binary speaks exactly that surface — nothing else — so the real client
//! transport ([`codex_agent`]) runs its spawn → handshake → thread → turn loop
//! end to end against a deterministic script instead of a model.
//!
//! It is the app-server analogue of `fake-claude`: where `fake-claude` scripts a
//! tmux + hooks + JSONL conversation, `fake-codex` scripts a JSON-RPC one. It is
//! reactive — it answers each incoming request by method and, on `turn/start`,
//! plays a scripted sequence of `item/*` / `turn/*` notifications (and,
//! optionally, a `*/requestApproval` server → client request).
//!
//! See [`scenario`] for the script vocabulary and how a scenario is selected.

mod scenario;
mod server;

use std::process::ExitCode;

fn main() -> ExitCode {
    match server::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // stderr is the fake's only diagnostic channel (stdout carries the
            // JSON-RPC frames), so a launch/scenario problem must surface there.
            eprintln!("fake-codex: {message}");
            ExitCode::FAILURE
        }
    }
}

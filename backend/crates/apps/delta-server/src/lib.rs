//! Delta server library.
//!
//! Hosts the control plane (Claude Code HTTP hooks), the browser REST surface
//! (`/api/*`), a browser event WebSocket (`/ws`), and a PTY bridge that attaches
//! an xterm.js terminal to the tmux pane (`/pty`). The binary in `main.rs` is a
//! thin wrapper that binds a listener and serves [`router`]; everything testable
//! lives here so integration tests can drive [`router`] directly.

mod api;
mod app;
mod hooks;
mod pty;
mod state;
mod version;
mod ws;

pub use app::router;
pub use state::AppState;
pub use version::display_version;

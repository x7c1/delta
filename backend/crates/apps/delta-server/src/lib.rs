//! Delta server library.
//!
//! Hosts the control plane (Claude Code HTTP hooks), the browser REST surface
//! (`/api/*`), a browser event WebSocket (`/ws`), a PTY bridge that attaches an
//! xterm.js terminal to the tmux pane (`/pty`), and the comms-log stream that
//! gives a terminal-less session the equivalent window (`/comms`). The binary in
//! `main.rs` is a thin wrapper that binds a listener and serves [`router`];
//! everything testable lives here so integration tests can drive [`router`]
//! directly. The [`config`] and [`serve`] modules hold the startup both the
//! binary and the desktop shell (`delta-desktop`) run.

mod api;
mod app;
mod auth_guard;
mod claude_version;
mod comms;
mod comms_log;
pub mod config;
mod hook_auth_guard;
mod hooks;
mod origin_guard;
mod pty;
mod route_binder;
pub mod serve;
mod state;
mod version;
mod ws;

pub use app::router;
pub use claude_version::log_claude_version;
pub use comms_log::{CommsLogHub, CommsSubscription, COMMS_RING_CAPACITY};
pub use state::AppState;
pub use version::display_version;

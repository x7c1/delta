//! tmux-backed [`TmuxDriver`] implementation.
//!
//! [`Tmux`] manages Claude Code sessions by shelling out to `tmux`. It is
//! stateless: every method takes the target session name (or pane) explicitly,
//! so one driver instance manages any number of concurrent sessions. It can
//! create/check/kill a session and send keystrokes: text is sent literally
//! (with `-l`) followed by a separate `Enter` keystroke so the prompt is
//! submitted exactly as typed.

mod error;
mod tmux;

pub use error::{Error, Result};
pub use tmux::Tmux;

/// The `tmux` program every command in this crate is spawned from.
///
/// Delta does not bundle tmux: it drives whatever `tmux` the host has on
/// `PATH`, and there is no environment override for the program name
/// (`DELTA_TMUX_SOCKET` picks the socket, not the binary). Exported so the
/// composition root can probe for the very same name at startup instead of
/// restating the literal and risking a probe that checks a different command
/// than the spawn runs.
pub const TMUX_BIN: &str = "tmux";

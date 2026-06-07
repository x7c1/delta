//! tmux-backed [`TmuxDriver`] implementation.
//!
//! [`Tmux`] manages the Claude Code session by shelling out to `tmux`. It is
//! constructed with a session name and derives the pane it drives as
//! `<session>:0.0`. It can create/check/kill the session and send keystrokes:
//! text is sent literally (with `-l`) followed by a separate `Enter` keystroke
//! so the prompt is submitted exactly as typed.

mod error;
mod tmux;

pub use error::{Error, Result};
pub use tmux::Tmux;

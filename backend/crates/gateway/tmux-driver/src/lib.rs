//! tmux-backed [`TmuxDriver`] implementation.
//!
//! [`Tmux`] drives the Claude Code session by shelling out to
//! `tmux send-keys -t <pane> ...`. The target pane is fixed at construction.
//! Text is sent literally (with `-l`) followed by a separate `Enter` keystroke
//! so the prompt is submitted exactly as typed.

mod error;
mod tmux;

pub use error::{Error, Result};
pub use tmux::Tmux;

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

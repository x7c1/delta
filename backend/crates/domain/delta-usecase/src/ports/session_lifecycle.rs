//! The outcome of ensuring the Claude Code session is up.

/// The state of the Claude Code session after an `ensure_session` call.
///
/// This describes the *tmux/process* lifecycle, not the conversational
/// [`delta_model::SessionStatus`] (which tracks whether a registered session is
/// active or ended). It tells the browser whether the session is already running
/// or was just spawned and may still be coming up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycle {
    /// The session already existed and was reused.
    Ready,
    /// The session was just created and may still be starting up.
    Starting,
}

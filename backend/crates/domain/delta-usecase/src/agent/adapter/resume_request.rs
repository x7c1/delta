//! Inputs for resuming a previously-closed agent session.

#[cfg(doc)]
use crate::LaunchRequest;

/// Inputs for resuming a previously-closed agent session.
#[derive(Debug, Clone)]
pub struct ResumeRequest {
    /// Delta's own conversation id.
    pub session_id: String,
    /// The provider's id for the session being resumed (Claude passes it to
    /// `--resume <id>`).
    pub provider_session_id: String,
    /// The working directory to resume the agent in.
    pub workdir: String,
    /// The root of the repository this session's worktree was cut from, when
    /// [`Self::workdir`] is a git worktree Delta created for the session;
    /// `None` otherwise. Re-derived on every resume from the session row Delta
    /// wrote at spawn — the same value the original [`LaunchRequest`] carried —
    /// so a resumed worktree session is configured exactly like a fresh one.
    /// See the `adapter` module docs' *The worktree's source repository*.
    pub worktree_repo_root: Option<String>,
}

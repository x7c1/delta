//! Inputs for launching a fresh agent session.

use super::LaunchOptionSpec;

/// Inputs for launching a fresh agent session.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    /// Delta's own conversation id. Providers that let Delta pin the identity
    /// (see [`crate::agent::SessionIdentityCapability::DeltaCanSetId`]) use it as the
    /// provider session id; others ignore it and return their own.
    pub session_id: String,
    /// The working directory the agent runs in.
    pub workdir: String,
    /// The launch options the user selected, in selection order, as neutral
    /// `(name, value?)` pairs. The adapter renders them for its provider —
    /// Claude appends them to the launch argv, Codex maps them onto
    /// `thread/start` fields. Empty when none were selected.
    pub launch_options: Vec<LaunchOptionSpec>,
    /// A first prompt delivered at launch, when the session was started from
    /// the composer's first Send.
    pub first_prompt: Option<String>,
    /// The root of the repository this session's worktree was cut from, when
    /// [`Self::workdir`] is a git worktree Delta created for the session;
    /// `None` for a session launched directly in a user-selected directory.
    /// See the `adapter` module docs' *The worktree's source repository* for
    /// why an adapter is told this.
    pub worktree_repo_root: Option<String>,
}

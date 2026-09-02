//! The fields the eager `spawning` row is inserted with.

use delta_model::{AgentProvider, SessionId};

/// What [`crate::ports::SessionStore::insert_spawning_session`] records: the
/// spawn-time snapshot a fresh session starts from, before its process has
/// reported anything.
#[derive(Debug, Clone)]
pub struct SpawningSession<'a> {
    /// The freshly-minted id the session is launched under.
    pub id: &'a SessionId,
    /// The directory the agent process is started in. For a worktree spawn
    /// this is the auto-generated worktree path, not the dir the user picked
    /// (that is `requested_workdir`).
    pub cwd: &'a str,
    /// Spawn-time snapshot of the local git branch checked out at `cwd`.
    /// `None` when the launch directory is not inside a git repository or HEAD
    /// was detached. Persisted once and never updated later: see
    /// [`delta_model::Session::branch_at_launch`].
    pub branch_at_launch: Option<&'a str>,
    /// Spawn-time snapshot of the repository root the spawn resolved against
    /// the dir the user picked: for a worktree spawn that is the repository
    /// the worktree was cut from, which does not contain `cwd`. `None` when
    /// the launch directory is not inside a git repository. See
    /// [`delta_model::Session::repo_root`].
    pub repo_root: Option<&'a str>,
    /// The dir the user picked, before any worktree resolution. `None` when no
    /// workdir was selected (the default per-token scratch dir is used). For a
    /// worktree-on spawn it holds the user-selected dir (the worktree's repo
    /// root); for a plain spawn with a user-selected workdir it equals `cwd`.
    /// See [`delta_model::Session::requested_workdir`].
    pub requested_workdir: Option<&'a str>,
    /// The cross-worktree repository identity label (`org/repo` from the
    /// `origin` URL, or the working-tree basename when no origin is set).
    /// `None` when the launch directory is not inside a git repository.
    /// Persisted once and never updated later: see
    /// [`delta_model::Session::repository_display_name`].
    pub repository_display_name: Option<&'a str>,
    /// The AI-agent backend the session runs on, recorded in the
    /// `session.provider` column. Every Claude spawn passes
    /// [`AgentProvider::Claude`] (the historical default); a structured
    /// provider such as Codex passes its own value. The provider-minted
    /// conversation ids are not known yet at spawn time — they are learned
    /// from the provider's launch response and written later via
    /// [`crate::ports::SessionStore::set_provider_ids`].
    pub provider: AgentProvider,
    /// The GitHub pull request the session was opened from (the new-session
    /// screen's PR tab), or `None` for every other origin. Like the git
    /// snapshot above it is written once and never updated on resume: see
    /// [`delta_model::Session::pull_request_number`].
    pub pull_request_number: Option<i64>,
}

//! The fields needed to register the session on first contact.

use delta_model::SessionId;

/// The fields needed to register the session on first contact.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: SessionId,
    pub cwd: String,
    pub transcript_path: String,
    /// Spawn-time snapshot of the launch directory's local git branch.
    /// `None` when the launch directory is not inside a git repository or
    /// HEAD was detached. See [`delta_model::Session::branch_at_launch`].
    pub branch_at_launch: Option<String>,
    /// Spawn-time snapshot of the working-tree root containing the launch
    /// directory. `None` when the launch directory is not inside a git
    /// repository. See [`delta_model::Session::repo_root`].
    pub repo_root: Option<String>,
    /// Spawn-time short repository identity label (e.g. `org/repo`). `None`
    /// when the launch directory is not inside a git repository, or when no
    /// origin URL is configured and no fallback could be computed. See
    /// [`delta_model::Session::repository_display_name`].
    pub repository_display_name: Option<String>,
}

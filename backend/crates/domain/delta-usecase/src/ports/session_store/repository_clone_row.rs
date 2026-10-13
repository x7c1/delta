//! One row of the Repository tab's aggregation.

/// One row of the Repository tab's aggregation: a single `(repo_root, clone)`
/// pair drawn from the session history, carrying enough state for the
/// interactor to bundle clones into repositories.
///
/// - `repo_root`: the repository root the clone lives under. Sessions launched
///   outside any git repo (no `session.repo_root`) never contribute — the
///   Repository tab is by definition a list of git repositories.
/// - `clone_path`: the dir the user picked at spawn time
///   (`session.requested_workdir`, falling back to `session.cwd` for sessions
///   that predate that column).
/// - `last_opened_at`: the most recent activity at this `(repo_root, clone)`
///   pair — `MAX(COALESCE(last_activity_at, created_at))` over its sessions.
/// - `last_branch`: the `branch_at_launch` of the latest session at this pair,
///   when one was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryCloneRow {
    pub repo_root: String,
    pub clone_path: String,
    pub last_opened_at: Option<String>,
    pub last_branch: Option<String>,
}

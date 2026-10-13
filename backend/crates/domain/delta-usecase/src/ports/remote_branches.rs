//! The remote branches of a repository.

#[cfg(doc)]
use super::GitWorktree;

/// The remote branches of a repository, with its default branch highlighted.
///
/// Returned by [`GitWorktree::fetch_remote_branches`] to back the branch picker:
/// `branches` are the remote branch short names (no `origin/` prefix), and
/// `default_branch` is the repository's default branch short name when it can be
/// determined (`None` when `origin/HEAD` is unset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranches {
    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// `origin/HEAD` is unset.
    pub default_branch: Option<String>,
    /// The remote branch short names (no `origin/` prefix), excluding the
    /// `origin/HEAD` symref entry.
    pub branches: Vec<String>,
}

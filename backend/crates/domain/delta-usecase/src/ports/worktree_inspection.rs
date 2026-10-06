//! What git says about a directory as a linked worktree.

/// What [`GitWorktree::inspect_worktree`] found for a directory git knows as a
/// worktree.
///
/// [`GitWorktree::inspect_worktree`]: crate::ports::GitWorktree::inspect_worktree
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInspection {
    /// The repository's main working tree — where `git worktree prune` runs
    /// after the worktree is removed.
    pub repo_root: String,
    /// Whether the worktree has modified, staged or untracked files: the work
    /// a plain `git worktree remove` refuses to destroy.
    pub dirty: bool,
}

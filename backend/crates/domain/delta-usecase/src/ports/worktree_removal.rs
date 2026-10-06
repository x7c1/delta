//! The outcome of asking git to remove a worktree without forcing it.

/// What [`GitWorktree::remove_worktree`] did with a worktree.
///
/// Only git's own *refusal* to remove a worktree that holds work is a value
/// here; any other failure (the repository is gone, `git` is missing) is an
/// error, so a caller can tell "kept because it holds work" from "could not
/// even ask".
///
/// [`GitWorktree::remove_worktree`]: crate::ports::GitWorktree::remove_worktree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeRemoval {
    /// `git worktree remove` deleted the worktree's directory and its
    /// administrative entry in the repository.
    Removed,
    /// git refused because the worktree has modified or untracked files; the
    /// worktree is left exactly as it was.
    KeptDirty,
}

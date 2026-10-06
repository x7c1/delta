//! The outcome of asking git to delete a branch only if it is merged.

/// What [`GitWorktree::delete_branch_if_merged`] did with a local branch.
///
/// As with [`WorktreeRemoval`], only git's refusal to delete a branch that
/// holds work is a value; any other failure is an error.
///
/// [`GitWorktree::delete_branch_if_merged`]: crate::ports::GitWorktree::delete_branch_if_merged
/// [`WorktreeRemoval`]: crate::ports::WorktreeRemoval
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchDeletion {
    /// `git branch -d` deleted the branch.
    Deleted,
    /// git refused because the branch is not merged into its upstream (or into
    /// `HEAD` when it has none); the branch is left as it was.
    KeptUnmerged,
    /// No local branch of that name exists, so there was nothing to delete.
    Absent,
}

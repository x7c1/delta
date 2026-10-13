//! A directory directly under the worktree base, as Settings → Storage lists it.

use crate::WorktreeBranch;

/// One directory under the worktree base: a worktree Delta created for a
/// session, or one left behind when its session was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDir {
    /// The directory's path, spelled under the configured worktree base.
    pub path: String,
    /// Whether a session Delta still lists works in it (its working
    /// directory, requested directory, or a message's directory). Such a
    /// worktree is not removable from Storage; removing its session is the
    /// way to clean it up.
    pub in_use: bool,
    /// The repository git reports the worktree belongs to, or `None` when git
    /// does not know the directory as a worktree.
    pub repo_root: Option<String>,
    /// Whether it has uncommitted or untracked changes, or `None` when git
    /// does not know the directory as a worktree.
    pub dirty: Option<bool>,
    /// The branch checked out in it, or `None` when git does not know the
    /// directory as a worktree or its `HEAD` is detached.
    pub branch: Option<WorktreeBranch>,
    /// The `delta-<id>` branch Delta cut for the session that created this
    /// worktree, when that session, still listed, has since checked out
    /// another branch here and the `delta-<id>` branch still exists. Removing
    /// the session settles it as it would the checked-out one (it is always
    /// `created_for_session`), so it is a branch that can stay too. `None`
    /// otherwise, including when git does not know the directory.
    pub launch_branch: Option<WorktreeBranch>,
}

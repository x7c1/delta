//! A directory directly under the worktree base, as Settings → Storage lists it.

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
}

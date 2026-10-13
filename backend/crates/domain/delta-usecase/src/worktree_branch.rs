//! A branch of a worktree under the worktree base.

/// A branch of a worktree under the worktree base — the one checked out in
/// it, or the one Delta cut for its session before the session switched away
/// — with what decides whether removing its session would delete it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeBranch {
    /// The branch's short name.
    pub name: String,
    /// Whether it is the `delta-<id>` branch Delta cut for session `<id>`,
    /// which Delta still lists and which works in this worktree: the only
    /// kind of branch removing a session (one at a time, in bulk, or by
    /// erasing everything) ever deletes. Any other branch — one a session was
    /// started on, or one whose session is already removed — stays.
    pub created_for_session: bool,
    /// Whether it is merged, as removing its session judges it
    /// (`git branch -d`). Asked only for a branch created for a session in a
    /// clean worktree — the one case where the answer decides anything — and
    /// `None` otherwise.
    pub merged: Option<bool>,
}

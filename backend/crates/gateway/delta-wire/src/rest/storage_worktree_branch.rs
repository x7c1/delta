//! A branch of a worktree under the worktree base, for
//! `GET /api/storage/worktrees`.

use delta_usecase::WorktreeBranch;
use serde::Serialize;
use ts_rs::TS;

/// A branch of a worktree (its `branch` or `launch_branch`), with what decides
/// whether removing its session deletes it.
///
/// `created_for_session` says it is the `delta-<id>` branch Delta cut for a
/// session it still lists that works in this worktree — the only kind of
/// branch removing a session, or erasing everything, ever deletes; any other
/// branch stays. `merged` is whether it is merged (`git branch -d` would
/// delete it), given only for such a branch in a clean worktree and `null`
/// otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageWorktreeBranch")]
pub struct WireStorageWorktreeBranch {
    pub name: String,
    pub created_for_session: bool,
    pub merged: Option<bool>,
}

impl From<WorktreeBranch> for WireStorageWorktreeBranch {
    fn from(branch: WorktreeBranch) -> Self {
        Self {
            name: branch.name,
            created_for_session: branch.created_for_session,
            merged: branch.merged,
        }
    }
}

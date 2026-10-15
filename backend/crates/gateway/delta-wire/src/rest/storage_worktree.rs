//! One directory under the worktree base, for `GET /api/storage/worktrees`.

use delta_usecase::WorktreeDir;
use serde::Serialize;
use ts_rs::TS;

use super::WireStorageWorktreeBranch;

/// A directory directly under the worktree base.
///
/// `in_use` says a listed session still works in it, so it is not removable
/// from Storage. `repo_root` and `dirty` are what git says of it; both are
/// `null` when git no longer knows the directory as a worktree (its repository
/// forgot it or is gone). `branch` is the branch checked out in it, `null`
/// for such a directory or a detached `HEAD`. `launch_branch` is the
/// `delta-<id>` branch Delta cut for the session that created it, when that
/// session has since checked out another branch there and the branch still
/// exists; `null` otherwise. `modified_at` is when the directory was last
/// modified (ISO-8601 UTC; it moves when an entry directly in it changes, not
/// when a file deeper down is edited), `null` when its metadata cannot be
/// read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageWorktree")]
pub struct WireStorageWorktree {
    pub path: String,
    pub in_use: bool,
    pub repo_root: Option<String>,
    pub dirty: Option<bool>,
    pub branch: Option<WireStorageWorktreeBranch>,
    pub launch_branch: Option<WireStorageWorktreeBranch>,
    pub modified_at: Option<String>,
}

impl From<WorktreeDir> for WireStorageWorktree {
    fn from(dir: WorktreeDir) -> Self {
        Self {
            path: dir.path,
            in_use: dir.in_use,
            repo_root: dir.repo_root,
            dirty: dir.dirty,
            branch: dir.branch.map(WireStorageWorktreeBranch::from),
            launch_branch: dir.launch_branch.map(WireStorageWorktreeBranch::from),
            modified_at: dir.modified_at,
        }
    }
}

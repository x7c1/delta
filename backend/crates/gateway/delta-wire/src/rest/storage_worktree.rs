//! One directory under the worktree base, for `GET /api/storage/worktrees`.

use delta_usecase::WorktreeDir;
use serde::Serialize;
use ts_rs::TS;

/// A directory directly under the worktree base.
///
/// `in_use` says a listed session still works in it, so it is not removable
/// from Storage. `repo_root` and `dirty` are what git says of it; both are
/// `null` when git no longer knows the directory as a worktree (its repository
/// forgot it or is gone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageWorktree")]
pub struct WireStorageWorktree {
    pub path: String,
    pub in_use: bool,
    pub repo_root: Option<String>,
    pub dirty: Option<bool>,
}

impl From<WorktreeDir> for WireStorageWorktree {
    fn from(dir: WorktreeDir) -> Self {
        Self {
            path: dir.path,
            in_use: dir.in_use,
            repo_root: dir.repo_root,
            dirty: dir.dirty,
        }
    }
}

//! One clone of a repository.

use delta_usecase::Clone as DomainClone;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::worktree_start_point::WireWorktreeStartPoint;

/// One clone of a repository: its absolute path on disk and the per-clone
/// state derived from the session history at that path.
///
/// Phase B does not yet persist per-session launch-option selections or
/// worktree state, so `last_launch_option_ids` is always empty,
/// `last_worktree_enabled` is always `false`, and
/// `last_worktree_start_point` is always `null`. A follow-up PR will start
/// recording those and the picker will pre-fill from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoryClone")]
pub struct WireRepositoryClone {
    pub path: String,
    /// ISO-8601 UTC timestamp of the most recent activity at this clone, or
    /// `null` when no contributing session has any activity yet.
    pub last_opened_at: Option<String>,
    /// Local branch the most recent session at this clone was launched on,
    /// when one was recorded.
    pub last_branch: Option<String>,
    pub last_launch_option_ids: Vec<i64>,
    pub last_worktree_enabled: bool,
    pub last_worktree_start_point: Option<WireWorktreeStartPoint>,
}

impl From<DomainClone> for WireRepositoryClone {
    fn from(clone: DomainClone) -> Self {
        WireRepositoryClone {
            path: clone.path,
            last_opened_at: clone.last_opened_at,
            last_branch: clone.last_branch,
            last_launch_option_ids: clone.last_launch_option_ids,
            last_worktree_enabled: clone.last_worktree_enabled,
            last_worktree_start_point: clone.last_worktree_start_point.map(Into::into),
        }
    }
}

//! Where a new session's worktree branch should start from.

use delta_usecase::WorktreeStartPoint;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a new session's worktree branch should start from.
///
/// Wire twin of the domain [`WorktreeStartPoint`], internally tagged by `kind`
/// (mirroring the `/ws` event union): `{ "kind": "head" }` cuts a new branch off
/// the repository's current `HEAD`, `{ "kind": "remote_branch", "name": "..." }`
/// cuts a new branch off the named remote branch (fetched first), and
/// `{ "kind": "use_remote_branch", "name": "..." }` works on the named branch
/// itself in the worktree (reusing the worktree that already has it checked out,
/// or creating one that checks it out).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(rename = "WorktreeStartPoint")]
pub enum WireWorktreeStartPoint {
    /// Cut a new branch off the repository's current `HEAD`.
    Head,
    /// Cut a new branch off `origin/<name>`, fetched first. `name` is the remote
    /// branch short name (no `origin/` prefix).
    RemoteBranch { name: String },
    /// Work on the branch `<name>` itself in the worktree (no `delta-<id>`
    /// branch). `name` is the branch short name (no `origin/` prefix).
    UseRemoteBranch { name: String },
}

impl From<WireWorktreeStartPoint> for WorktreeStartPoint {
    fn from(start_point: WireWorktreeStartPoint) -> Self {
        match start_point {
            WireWorktreeStartPoint::Head => WorktreeStartPoint::Head,
            WireWorktreeStartPoint::RemoteBranch { name } => WorktreeStartPoint::RemoteBranch(name),
            WireWorktreeStartPoint::UseRemoteBranch { name } => {
                WorktreeStartPoint::UseRemoteBranch(name)
            }
        }
    }
}

impl From<WorktreeStartPoint> for WireWorktreeStartPoint {
    fn from(start_point: WorktreeStartPoint) -> Self {
        match start_point {
            WorktreeStartPoint::Head => WireWorktreeStartPoint::Head,
            WorktreeStartPoint::RemoteBranch(name) => WireWorktreeStartPoint::RemoteBranch { name },
            WorktreeStartPoint::UseRemoteBranch(name) => {
                WireWorktreeStartPoint::UseRemoteBranch { name }
            }
        }
    }
}

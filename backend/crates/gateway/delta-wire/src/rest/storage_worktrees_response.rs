//! Response for `GET /api/storage/worktrees`.

use serde::Serialize;
use ts_rs::TS;

use super::WireStorageWorktree;

/// Response for `GET /api/storage/worktrees`: every directory directly under
/// the worktree base, sorted by name. The ones no listed session works in are
/// the leftovers `DELETE /api/storage/worktrees` can remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageWorktreesResponse")]
pub struct WireStorageWorktreesResponse {
    pub worktrees: Vec<WireStorageWorktree>,
}

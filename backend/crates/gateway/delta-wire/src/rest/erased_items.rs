//! What `POST /api/storage/erase` removed.

use serde::Serialize;
use ts_rs::TS;

/// What erasing everything removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "ErasedItems")]
pub struct WireErasedItems {
    /// How many sessions were removed.
    pub sessions: u32,
    /// The worktrees removed, the sessions' first and then the leftovers
    /// under the worktree base.
    pub worktrees: Vec<String>,
    /// The `delta-<session id>` branches deleted because they were merged.
    pub branches: Vec<String>,
    /// The data directory. Once it has stopped — after this response is sent —
    /// the server deletes Delta's files in it, and the directory itself only
    /// when nothing else is left in it.
    pub data_dir: String,
}

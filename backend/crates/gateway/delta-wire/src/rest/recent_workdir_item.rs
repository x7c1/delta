//! One recently-used working directory.

use delta_usecase::RecentWorkdir;
use serde::Serialize;
use ts_rs::TS;

/// One recently-used working directory: its absolute path and the timestamp of
/// the latest activity in any session that used it (ISO-8601 UTC, `null` when
/// unknown).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RecentWorkdirItem")]
pub struct WireRecentWorkdirItem {
    pub path: String,
    pub last_used_at: Option<String>,
}

impl From<RecentWorkdir> for WireRecentWorkdirItem {
    fn from((path, last_used_at): RecentWorkdir) -> Self {
        WireRecentWorkdirItem { path, last_used_at }
    }
}

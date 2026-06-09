//! Response for `GET /api/workdir/recent`.

use serde::Serialize;

use delta_usecase::RecentWorkdir;

/// One recently-used working directory: its absolute path and the timestamp of
/// the latest activity in any session that used it (ISO-8601 UTC, `null` when
/// unknown).
#[derive(Debug, Serialize)]
pub struct RecentWorkdirItem {
    pub path: String,
    pub last_used_at: Option<String>,
}

impl From<RecentWorkdir> for RecentWorkdirItem {
    fn from((path, last_used_at): RecentWorkdir) -> Self {
        RecentWorkdirItem { path, last_used_at }
    }
}

/// Response for `GET /api/workdir/recent`: recently-used working directories,
/// most-recent first.
#[derive(Debug, Serialize)]
pub struct WorkdirRecentResponse {
    pub workdirs: Vec<RecentWorkdirItem>,
}

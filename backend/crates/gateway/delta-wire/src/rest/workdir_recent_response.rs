//! Response for `GET /api/workdir/recent`.

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

/// Response for `GET /api/workdir/recent`: recently-used working directories,
/// most-recent first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "WorkdirRecentResponse")]
pub struct WireWorkdirRecentResponse {
    pub workdirs: Vec<WireRecentWorkdirItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_workdirs_serialize_with_the_rest_field_names() {
        assert_eq!(
            serde_json::to_value(WireWorkdirRecentResponse {
                workdirs: vec![WireRecentWorkdirItem::from((
                    "/work".to_owned(),
                    Some("2026-01-01T00:00:00Z".to_owned()),
                ))],
            })
            .unwrap(),
            serde_json::json!({
                "workdirs": [{ "path": "/work", "last_used_at": "2026-01-01T00:00:00Z" }],
            }),
        );
    }
}

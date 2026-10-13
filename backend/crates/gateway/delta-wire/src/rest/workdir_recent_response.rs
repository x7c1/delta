//! Response for `GET /api/workdir/recent`.

use serde::Serialize;
use ts_rs::TS;

use crate::rest::recent_workdir_item::WireRecentWorkdirItem;

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

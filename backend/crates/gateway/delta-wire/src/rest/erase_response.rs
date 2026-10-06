//! Response for `POST /api/storage/erase`.

use delta_usecase::EraseReport;
use serde::Serialize;
use ts_rs::TS;

use super::{WireErasedItems, WireKeptItem};

/// Response for `POST /api/storage/erase`: what erasing everything removed,
/// and what it kept because it held work.
///
/// `kept` is shaped as the bulk session removal's, so one component renders
/// both: the items the removed sessions kept carry their `session_id`, the
/// leftovers under the worktree base carry `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "EraseResponse")]
pub struct WireEraseResponse {
    pub removed: WireErasedItems,
    pub kept: Vec<WireKeptItem>,
}

impl WireEraseResponse {
    /// The response for `report`, with the data directory the server empties
    /// once it has stopped.
    pub fn new(report: EraseReport, data_dir: impl Into<String>) -> Self {
        let removed = WireErasedItems {
            sessions: u32::try_from(report.removed_sessions.len()).unwrap_or(u32::MAX),
            worktrees: report.removed_worktrees().map(str::to_owned).collect(),
            branches: report.removed_branches().map(str::to_owned).collect(),
            data_dir: data_dir.into(),
        };
        let kept = report
            .kept
            .into_iter()
            .map(WireKeptItem::from)
            .chain(report.kept_leftovers.into_iter().map(WireKeptItem::from))
            .collect();
        Self { removed, kept }
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::{DiskItem, KeepReason, KeptItem, SessionId, SessionKeptItem};

    use super::*;

    #[test]
    fn a_report_serializes_with_the_session_kept_items_then_the_leftovers() {
        let report = EraseReport {
            removed_sessions: vec![SessionId::from("a"), SessionId::from("b")],
            removed: vec![
                DiskItem::Worktree("/w/a".into()),
                DiskItem::TrustEntry("/w/a".into()),
                DiskItem::Branch("delta-a".into()),
                DiskItem::Worktree("/w/left".into()),
            ],
            kept: vec![SessionKeptItem {
                session_id: SessionId::from("b"),
                kept: KeptItem {
                    item: DiskItem::Branch("delta-b".into()),
                    reason: KeepReason::Unmerged,
                },
            }],
            kept_leftovers: vec![KeptItem {
                item: DiskItem::Worktree("/w/dirty".into()),
                reason: KeepReason::Dirty,
            }],
        };

        let value = serde_json::to_value(WireEraseResponse::new(report, "/data")).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "removed": {
                    "sessions": 2,
                    "worktrees": ["/w/a", "/w/left"],
                    "branches": ["delta-a"],
                    "data_dir": "/data",
                },
                "kept": [
                    {
                        "session_id": "b",
                        "kind": "branch",
                        "target": "delta-b",
                        "reason": "unmerged",
                        "detail": null,
                    },
                    {
                        "session_id": null,
                        "kind": "worktree",
                        "target": "/w/dirty",
                        "reason": "dirty",
                        "detail": null,
                    },
                ],
            })
        );
    }
}

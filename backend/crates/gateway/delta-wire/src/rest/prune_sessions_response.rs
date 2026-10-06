//! Response for `POST /api/sessions/prune`.

use delta_usecase::PruneReport;
use serde::Serialize;
use ts_rs::TS;

use super::{WireKeptItem, WireSkippedSession};

/// Response for `POST /api/sessions/prune`: what the bulk removal did.
///
/// Every matching session is either in `removed_ids` or in `skipped`. `kept`
/// lists what the removed sessions left on disk because it held work, exactly
/// as removing each one alone would have kept it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PruneSessionsResponse")]
pub struct WirePruneSessionsResponse {
    /// How many sessions were removed (`removed_ids.len()`).
    pub removed: u32,
    /// The removed sessions, oldest first.
    pub removed_ids: Vec<String>,
    pub skipped: Vec<WireSkippedSession>,
    pub kept: Vec<WireKeptItem>,
}

impl From<PruneReport> for WirePruneSessionsResponse {
    fn from(report: PruneReport) -> Self {
        Self {
            removed: u32::try_from(report.removed.len()).unwrap_or(u32::MAX),
            removed_ids: report
                .removed
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            skipped: report
                .skipped
                .into_iter()
                .map(WireSkippedSession::from)
                .collect(),
            kept: report.kept.into_iter().map(WireKeptItem::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::{
        DiskItem, KeepReason, KeptItem, SessionId, SessionKeptItem, SkipReason, SkippedSession,
    };

    use super::*;

    #[test]
    fn a_report_serializes_with_its_kept_items_and_skips() {
        let report = PruneReport {
            removed: vec![SessionId::from("a"), SessionId::from("b")],
            skipped: vec![
                SkippedSession {
                    session_id: SessionId::from("c"),
                    reason: SkipReason::Open,
                },
                SkippedSession {
                    session_id: SessionId::from("d"),
                    reason: SkipReason::Failed("boom".into()),
                },
            ],
            kept: vec![SessionKeptItem {
                session_id: SessionId::from("b"),
                kept: KeptItem {
                    item: DiskItem::Worktree("/w/b".into()),
                    reason: KeepReason::Dirty,
                },
            }],
        };

        let value = serde_json::to_value(WirePruneSessionsResponse::from(report)).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "removed": 2,
                "removed_ids": ["a", "b"],
                "skipped": [
                    { "session_id": "c", "reason": "open", "detail": null },
                    { "session_id": "d", "reason": "failed", "detail": "boom" },
                ],
                "kept": [{
                    "session_id": "b",
                    "kind": "worktree",
                    "target": "/w/b",
                    "reason": "dirty",
                    "detail": null,
                }],
            })
        );
    }
}

//! The wire form of [`AgentFileChangeDetail`]: what a pending permission
//! request would do to files on disk.
//!
//! Two surfaces carry it and must agree, because a client can arrive at the same
//! card either way: the `permission_requested` event (a live raise) and the
//! sends envelope's pending permission (the re-seed after a reconnect, when the
//! event was missed). Both go through this type and the per-file types it
//! carries, so the card cannot be rich on one path and bare on the other.

use delta_usecase::AgentFileChangeDetail;
use serde::Serialize;
use ts_rs::TS;

use crate::file_change::WireFileChange;

/// What a pending permission request would do to files on disk.
///
/// Present only when the request actually states the change set — never
/// synthesised empty — so a client can treat its absence as "nothing is known
/// here" and show the request's arguments alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "FileChangeDetail")]
pub struct WireFileChangeDetail {
    /// The files the request would change, in the order the provider listed
    /// them.
    pub changes: Vec<WireFileChange>,
    /// The provider's own explanation for why it is asking, when it offered one.
    pub reason: Option<String>,
}

impl From<&AgentFileChangeDetail> for WireFileChangeDetail {
    fn from(detail: &AgentFileChangeDetail) -> Self {
        WireFileChangeDetail {
            changes: detail.changes.iter().map(WireFileChange::from).collect(),
            reason: detail.reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_usecase::{AgentFileChange, AgentFileChangeKind};

    #[test]
    fn a_detail_serializes_with_its_paths_kinds_and_diffs() {
        let detail = AgentFileChangeDetail {
            changes: vec![
                AgentFileChange {
                    path: "src/main.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Update),
                    diff: "@@\n-a\n+b".to_owned(),
                },
                AgentFileChange {
                    path: "src/new.rs".to_owned(),
                    kind: None,
                    diff: String::new(),
                },
            ],
            reason: Some("needs write access".to_owned()),
        };

        assert_eq!(
            serde_json::to_value(WireFileChangeDetail::from(&detail)).expect("serializes"),
            serde_json::json!({
                "changes": [
                    { "path": "src/main.rs", "kind": "update", "diff": "@@\n-a\n+b" },
                    { "path": "src/new.rs", "kind": null, "diff": "" },
                ],
                "reason": "needs write access",
            }),
        );
    }
}

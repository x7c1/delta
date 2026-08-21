//! The wire form of [`AgentFileChangeDetail`]: what a pending permission
//! request would do to files on disk.
//!
//! Two surfaces carry it and must agree, because a client can arrive at the same
//! card either way: the `permission_requested` event (a live raise) and the
//! sends envelope's pending permission (the re-seed after a reconnect, when the
//! event was missed). Both go through the types here, so the card cannot be rich
//! on one path and bare on the other.

use delta_usecase::{AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind};
use serde::Serialize;
use ts_rs::TS;

/// How one file would change — the wire twin of [`AgentFileChangeKind`].
///
/// A closed, provider-neutral vocabulary: these are the three things a patch can
/// do to a path. A provider naming anything else reaches the client as no kind
/// at all (see [`WireFileChange::kind`]) rather than as a string the UI would
/// have to guess at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "FileChangeKind")]
pub enum WireFileChangeKind {
    /// The file does not exist yet and would be created.
    Add,
    /// An existing file's contents would be edited.
    Update,
    /// The file would be removed.
    Delete,
}

impl From<AgentFileChangeKind> for WireFileChangeKind {
    fn from(kind: AgentFileChangeKind) -> Self {
        match kind {
            AgentFileChangeKind::Add => WireFileChangeKind::Add,
            AgentFileChangeKind::Update => WireFileChangeKind::Update,
            AgentFileChangeKind::Delete => WireFileChangeKind::Delete,
        }
    }
}

/// One file a pending permission request would change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "FileChange")]
pub struct WireFileChange {
    /// The path that would change, exactly as the provider named it.
    pub path: String,
    /// How it would change, or `null` when the provider named a kind Delta does
    /// not model — the path and diff are still shown, without a kind label.
    pub kind: Option<WireFileChangeKind>,
    /// The unified diff of the proposed change, as the provider produced it.
    /// Shown behind an expand control rather than inline: it can be long, and
    /// the paths are what the answer usually turns on.
    pub diff: String,
}

impl From<&AgentFileChange> for WireFileChange {
    fn from(change: &AgentFileChange) -> Self {
        WireFileChange {
            path: change.path.clone(),
            kind: change.kind.map(WireFileChangeKind::from),
            diff: change.diff.clone(),
        }
    }
}

/// What a pending permission request would do to files on disk.
///
/// Present only when the provider actually stated the change set — never
/// synthesised empty — so a client can treat its absence as "nothing is known
/// here" and fall back to the request's input summary.
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

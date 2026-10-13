//! One file a pending permission request would change.

use delta_usecase::AgentFileChange;
use serde::Serialize;
use ts_rs::TS;

use crate::file_change_kind::WireFileChangeKind;

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

//! How one file would change — the wire twin of [`AgentFileChangeKind`].

use delta_usecase::AgentFileChangeKind;
use serde::Serialize;
use ts_rs::TS;

#[cfg(doc)]
use crate::file_change::WireFileChange;

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

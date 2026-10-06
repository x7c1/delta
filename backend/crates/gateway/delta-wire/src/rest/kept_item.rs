//! A piece of disk state a removed session kept, as
//! `POST /api/sessions/prune` reports it.

use delta_usecase::{DiskItem, KeepReason, SessionKeptItem};
use serde::Serialize;
use ts_rs::TS;

use super::{WireDiskItemKind, WireKeepReason};

/// A worktree, branch or trust entry that removing `session_id` kept on disk,
/// and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "KeptItem")]
pub struct WireKeptItem {
    pub session_id: String,
    pub kind: WireDiskItemKind,
    /// The worktree's path, the branch's name, or the trusted directory.
    pub target: String,
    pub reason: WireKeepReason,
    /// The error, for `reason: "failed"`; `null` otherwise.
    pub detail: Option<String>,
}

impl From<SessionKeptItem> for WireKeptItem {
    fn from(item: SessionKeptItem) -> Self {
        let kind = WireDiskItemKind::from(&item.kept.item);
        let reason = WireKeepReason::from(&item.kept.reason);
        Self {
            session_id: item.session_id.as_str().to_owned(),
            kind,
            target: match item.kept.item {
                DiskItem::Worktree(target)
                | DiskItem::Branch(target)
                | DiskItem::TrustEntry(target) => target,
            },
            reason,
            detail: match item.kept.reason {
                KeepReason::Failed(error) => Some(error),
                _ => None,
            },
        }
    }
}

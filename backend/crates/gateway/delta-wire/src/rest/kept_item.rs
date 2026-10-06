//! A piece of disk state Delta kept, as `POST /api/sessions/prune` and
//! `POST /api/storage/erase` report it.

use delta_usecase::{DiskItem, KeepReason, KeptItem, SessionKeptItem};
use serde::Serialize;
use ts_rs::TS;

use super::{WireDiskItemKind, WireKeepReason};

/// A worktree, branch or trust entry that removing a session, or erasing
/// everything, kept on disk, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "KeptItem")]
pub struct WireKeptItem {
    /// The removed session the item belonged to; `null` for a leftover
    /// directory under the worktree base, which belongs to no session.
    pub session_id: Option<String>,
    pub kind: WireDiskItemKind,
    /// The worktree's path, the branch's name, or the trusted directory.
    pub target: String,
    pub reason: WireKeepReason,
    /// The error, for `reason: "failed"`; `null` otherwise.
    pub detail: Option<String>,
}

impl WireKeptItem {
    fn new(session_id: Option<String>, kept: KeptItem) -> Self {
        Self {
            session_id,
            kind: WireDiskItemKind::from(&kept.item),
            reason: WireKeepReason::from(&kept.reason),
            target: match kept.item {
                DiskItem::Worktree(target)
                | DiskItem::Branch(target)
                | DiskItem::TrustEntry(target) => target,
            },
            detail: match kept.reason {
                KeepReason::Failed(error) => Some(error),
                _ => None,
            },
        }
    }
}

impl From<SessionKeptItem> for WireKeptItem {
    fn from(item: SessionKeptItem) -> Self {
        Self::new(Some(item.session_id.as_str().to_owned()), item.kept)
    }
}

/// A kept item of no session: a leftover under the worktree base.
impl From<KeptItem> for WireKeptItem {
    fn from(kept: KeptItem) -> Self {
        Self::new(None, kept)
    }
}

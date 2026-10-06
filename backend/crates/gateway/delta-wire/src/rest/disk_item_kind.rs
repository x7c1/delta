//! What kind of disk state a removed session left behind.

use delta_usecase::DiskItem;
use serde::Serialize;
use ts_rs::TS;

/// The kind of a piece of disk state Delta created for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "DiskItemKind")]
pub enum WireDiskItemKind {
    /// A git worktree; the target is its path.
    Worktree,
    /// A local git branch; the target is its name.
    Branch,
    /// Claude Code's trust entry for a directory in `~/.claude.json`; the
    /// target is the directory.
    TrustEntry,
}

impl From<&DiskItem> for WireDiskItemKind {
    fn from(item: &DiskItem) -> Self {
        match item {
            DiskItem::Worktree(_) => Self::Worktree,
            DiskItem::Branch(_) => Self::Branch,
            DiskItem::TrustEntry(_) => Self::TrustEntry,
        }
    }
}

//! One registered clone root.

use delta_usecase::CloneRoot;
use serde::Serialize;
use ts_rs::TS;

/// One registered clone root: just the absolute path. The `created_at`
/// timestamp is omitted from the wire because the client only needs the path to
/// render the row and to address the DELETE endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "CloneRoot")]
pub struct WireCloneRoot {
    pub path: String,
}

impl From<CloneRoot> for WireCloneRoot {
    fn from(root: CloneRoot) -> Self {
        WireCloneRoot { path: root.path }
    }
}

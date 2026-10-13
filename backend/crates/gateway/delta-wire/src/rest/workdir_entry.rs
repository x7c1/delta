//! One subdirectory in a browse listing: its bare name and absolute path.

use delta_usecase::DirEntry;
use serde::Serialize;
use ts_rs::TS;

/// One subdirectory in a browse listing: its bare name and absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "WorkdirEntry")]
pub struct WireWorkdirEntry {
    pub name: String,
    pub path: String,
}

impl From<DirEntry> for WireWorkdirEntry {
    fn from(entry: DirEntry) -> Self {
        WireWorkdirEntry {
            name: entry.name,
            path: entry.path,
        }
    }
}

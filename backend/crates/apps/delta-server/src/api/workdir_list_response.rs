//! Response for `GET /api/workdir/list`.

use serde::Serialize;

use delta_usecase::{DirEntry, DirListing};

/// One subdirectory in a browse listing: its bare name and absolute path.
#[derive(Debug, Serialize)]
pub struct WorkdirEntry {
    pub name: String,
    pub path: String,
}

impl From<DirEntry> for WorkdirEntry {
    fn from(entry: DirEntry) -> Self {
        WorkdirEntry {
            name: entry.name,
            path: entry.path,
        }
    }
}

/// Response for `GET /api/workdir/list`: one level of a directory browse.
///
/// `path` is the canonical directory that was listed, `parent` its canonical
/// parent (`null` at a filesystem root), and `entries` its immediate
/// subdirectories (dirs only, dot-directories hidden, sorted by name).
#[derive(Debug, Serialize)]
pub struct WorkdirListResponse {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<WorkdirEntry>,
}

impl From<DirListing> for WorkdirListResponse {
    fn from(listing: DirListing) -> Self {
        WorkdirListResponse {
            path: listing.path,
            parent: listing.parent,
            entries: listing.entries.into_iter().map(WorkdirEntry::from).collect(),
        }
    }
}

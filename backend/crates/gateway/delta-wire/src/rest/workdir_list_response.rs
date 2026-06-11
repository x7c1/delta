//! Response for `GET /api/workdir/list`.

use delta_usecase::{DirEntry, DirListing};
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

/// Response for `GET /api/workdir/list`: one level of a directory browse.
///
/// `path` is the canonical directory that was listed, `parent` its canonical
/// parent (`null` at a filesystem root), and `entries` its immediate
/// subdirectories (dirs only, dot-directories hidden, sorted by name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "WorkdirListResponse")]
pub struct WireWorkdirListResponse {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<WireWorkdirEntry>,
}

impl From<DirListing> for WireWorkdirListResponse {
    fn from(listing: DirListing) -> Self {
        WireWorkdirListResponse {
            path: listing.path,
            parent: listing.parent,
            entries: listing
                .entries
                .into_iter()
                .map(WireWorkdirEntry::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_serializes_with_the_rest_field_names() {
        let listing = DirListing {
            path: "/home/user".into(),
            parent: Some("/home".into()),
            entries: vec![DirEntry {
                name: "projects".into(),
                path: "/home/user/projects".into(),
            }],
        };
        assert_eq!(
            serde_json::to_value(WireWorkdirListResponse::from(listing)).unwrap(),
            serde_json::json!({
                "path": "/home/user",
                "parent": "/home",
                "entries": [{ "name": "projects", "path": "/home/user/projects" }],
            }),
        );
    }
}

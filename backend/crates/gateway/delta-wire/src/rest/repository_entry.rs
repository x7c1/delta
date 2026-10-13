//! One repository in the Repository tab.

use delta_usecase::Repository;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::repository_clone::WireRepositoryClone;

/// One repository in the Repository tab: stable identity, display name, the
/// clone to default-select, and every known clone of this upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoryEntry")]
pub struct WireRepositoryEntry {
    /// Stable identity key: the normalised `origin` URL (e.g.
    /// `github.com/x7c1/delta`) when one is set, or the clone's absolute path
    /// when no origin was found.
    pub identity_key: String,
    /// Human-readable label for the picker (e.g. `x7c1/delta`).
    pub display_name: String,
    /// Path of the clone the picker pre-selects (the most recently used).
    pub recently_used_clone_path: String,
    /// All known clones of this repository, ordered most-recent first.
    pub clones: Vec<WireRepositoryClone>,
}

impl From<Repository> for WireRepositoryEntry {
    fn from(repo: Repository) -> Self {
        WireRepositoryEntry {
            identity_key: repo.identity_key,
            display_name: repo.display_name,
            recently_used_clone_path: repo.recently_used_clone_path,
            clones: repo.clones.into_iter().map(Into::into).collect(),
        }
    }
}

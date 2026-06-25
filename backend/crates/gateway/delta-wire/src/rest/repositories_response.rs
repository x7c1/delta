//! Response for `GET /api/repositories`: the Repository tab's recency-ordered
//! list of registered repositories, each bundling its known clones.

use delta_usecase::{Clone as DomainClone, Repository};
use serde::Serialize;
use ts_rs::TS;

use crate::rest::worktree_spec::WireWorktreeStartPoint;

/// One clone of a repository: its absolute path on disk and the per-clone
/// state derived from the session history at that path.
///
/// Phase B does not yet persist per-session launch-option selections or
/// worktree state, so `last_launch_option_ids` is always empty,
/// `last_worktree_enabled` is always `false`, and
/// `last_worktree_start_point` is always `null`. A follow-up PR will start
/// recording those and the picker will pre-fill from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoryClone")]
pub struct WireRepositoryClone {
    pub path: String,
    /// ISO-8601 UTC timestamp of the most recent activity at this clone, or
    /// `null` when no contributing session has any activity yet.
    pub last_opened_at: Option<String>,
    /// Local branch the most recent session at this clone was launched on,
    /// when one was recorded.
    pub last_branch: Option<String>,
    pub last_launch_option_ids: Vec<i64>,
    pub last_worktree_enabled: bool,
    pub last_worktree_start_point: Option<WireWorktreeStartPoint>,
}

impl From<DomainClone> for WireRepositoryClone {
    fn from(clone: DomainClone) -> Self {
        WireRepositoryClone {
            path: clone.path,
            last_opened_at: clone.last_opened_at,
            last_branch: clone.last_branch,
            last_launch_option_ids: clone.last_launch_option_ids,
            last_worktree_enabled: clone.last_worktree_enabled,
            last_worktree_start_point: clone.last_worktree_start_point.map(Into::into),
        }
    }
}

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

/// Response for `GET /api/repositories`: the Repository tab's list of
/// registered repositories, most-recently-active first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RepositoriesResponse")]
pub struct WireRepositoriesResponse {
    pub repositories: Vec<WireRepositoryEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_response_serialises_as_an_empty_array() {
        assert_eq!(
            serde_json::to_value(WireRepositoriesResponse {
                repositories: Vec::new(),
            })
            .unwrap(),
            serde_json::json!({ "repositories": [] }),
        );
    }

    #[test]
    fn one_repo_one_clone_round_trips_the_field_names() {
        let value = WireRepositoriesResponse {
            repositories: vec![WireRepositoryEntry {
                identity_key: "github.com/x7c1/delta".into(),
                display_name: "x7c1/delta".into(),
                recently_used_clone_path: "/work/delta".into(),
                clones: vec![WireRepositoryClone {
                    path: "/work/delta".into(),
                    last_opened_at: Some("2026-01-01T00:00:00Z".into()),
                    last_branch: Some("main".into()),
                    last_launch_option_ids: Vec::new(),
                    last_worktree_enabled: false,
                    last_worktree_start_point: None,
                }],
            }],
        };
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({
                "repositories": [{
                    "identity_key": "github.com/x7c1/delta",
                    "display_name": "x7c1/delta",
                    "recently_used_clone_path": "/work/delta",
                    "clones": [{
                        "path": "/work/delta",
                        "last_opened_at": "2026-01-01T00:00:00Z",
                        "last_branch": "main",
                        "last_launch_option_ids": [],
                        "last_worktree_enabled": false,
                        "last_worktree_start_point": null,
                    }],
                }],
            }),
        );
    }
}

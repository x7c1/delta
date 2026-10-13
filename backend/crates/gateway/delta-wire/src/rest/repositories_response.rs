//! Response for `GET /api/repositories`: the Repository tab's recency-ordered
//! list of registered repositories, each bundling its known clones.

use serde::Serialize;
use ts_rs::TS;

use crate::rest::repository_entry::WireRepositoryEntry;

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

    use crate::rest::repository_clone::WireRepositoryClone;

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

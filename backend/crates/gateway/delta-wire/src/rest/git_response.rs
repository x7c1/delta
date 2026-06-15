//! Responses for the git-repository detection endpoints.

use delta_usecase::{GitRepoInfo, RemoteBranches};
use serde::Serialize;
use ts_rs::TS;

/// Response for `GET /api/workdir/git`: whether a directory is a git repository.
///
/// `repo_root` is the repository root containing the queried path (`null` when
/// it is not inside a git repository), and `default_branch` is that
/// repository's default branch short name when known (`null` otherwise).
/// Computed without any network access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "GitRepoResponse")]
pub struct WireGitRepoResponse {
    pub repo_root: Option<String>,
    pub default_branch: Option<String>,
}

impl From<GitRepoInfo> for WireGitRepoResponse {
    fn from(info: GitRepoInfo) -> Self {
        WireGitRepoResponse {
            repo_root: info.repo_root,
            default_branch: info.default_branch,
        }
    }
}

/// Response for `GET /api/workdir/git/branches`: the remote branches of a
/// repository.
///
/// `default_branch` is the repository's default branch short name when known
/// (`null` otherwise), and `remote_branches` are the remote branch short names
/// (no `origin/` prefix), excluding the `origin/HEAD` symref. The list reflects
/// a fresh fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "GitBranchesResponse")]
pub struct WireGitBranchesResponse {
    pub default_branch: Option<String>,
    pub remote_branches: Vec<String>,
}

impl From<RemoteBranches> for WireGitBranchesResponse {
    fn from(remote: RemoteBranches) -> Self {
        WireGitBranchesResponse {
            default_branch: remote.default_branch,
            remote_branches: remote.branches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_response_serializes_with_rest_field_names() {
        let info = GitRepoInfo {
            repo_root: Some("/projects/app".into()),
            default_branch: Some("main".into()),
        };
        assert_eq!(
            serde_json::to_value(WireGitRepoResponse::from(info)).unwrap(),
            serde_json::json!({ "repo_root": "/projects/app", "default_branch": "main" }),
        );
    }

    #[test]
    fn repo_response_serializes_nulls_for_a_non_repo() {
        let info = GitRepoInfo {
            repo_root: None,
            default_branch: None,
        };
        assert_eq!(
            serde_json::to_value(WireGitRepoResponse::from(info)).unwrap(),
            serde_json::json!({ "repo_root": null, "default_branch": null }),
        );
    }

    #[test]
    fn branches_response_serializes_the_branch_list() {
        let remote = RemoteBranches {
            default_branch: Some("main".into()),
            branches: vec!["main".into(), "feature".into()],
        };
        assert_eq!(
            serde_json::to_value(WireGitBranchesResponse::from(remote)).unwrap(),
            serde_json::json!({
                "default_branch": "main",
                "remote_branches": ["main", "feature"],
            }),
        );
    }
}

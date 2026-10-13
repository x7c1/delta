//! Response for `GET /api/workdir/git`.

use delta_usecase::GitRepoInfo;
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
}

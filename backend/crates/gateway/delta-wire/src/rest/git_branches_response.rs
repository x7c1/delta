//! Response for `GET /api/workdir/git/branches`.

use delta_usecase::RemoteBranches;
use serde::Serialize;
use ts_rs::TS;

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

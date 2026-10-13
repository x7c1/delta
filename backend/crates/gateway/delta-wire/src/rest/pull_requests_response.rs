//! Response for `GET /api/prs?lens=…`: the new-session PR tab's per-lens
//! pull-request list, plus a flag telling the UI whether the `gh` CLI
//! could be used at all (so an unauthenticated host can render an inline
//! hint without the endpoint having to 5xx).

use delta_usecase::PullRequestList;
use serde::Serialize;
use ts_rs::TS;

use crate::rest::pull_request::WirePullRequest;

/// Response for `GET /api/prs?lens=…`.
///
/// `gh_available` is `false` when the `gh` CLI is missing or
/// `gh auth status` fails. In that case `pull_requests` is empty and the
/// endpoint still returns 200 — the PR tab renders an inline "run
/// `gh auth login`" hint rather than a generic failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PullRequestsResponse")]
pub struct WirePullRequestsResponse {
    pub gh_available: bool,
    pub pull_requests: Vec<WirePullRequest>,
}

impl From<PullRequestList> for WirePullRequestsResponse {
    fn from(list: PullRequestList) -> Self {
        WirePullRequestsResponse {
            gh_available: list.gh_available,
            pull_requests: list.pull_requests.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use delta_usecase::PullRequest as DomainPullRequest;

    #[test]
    fn an_unauthenticated_list_serialises_with_an_empty_array_and_the_flag_off() {
        let value = WirePullRequestsResponse::from(PullRequestList {
            gh_available: false,
            pull_requests: Vec::new(),
        });
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({
                "gh_available": false,
                "pull_requests": [],
            }),
        );
    }

    #[test]
    fn one_pr_round_trips_the_field_names() {
        let value = WirePullRequestsResponse::from(PullRequestList {
            gh_available: true,
            pull_requests: vec![DomainPullRequest {
                number: 42,
                title: "feat: x".into(),
                repo_owner: "x7c1".into(),
                repo_name: "delta".into(),
                head_ref: "feat/x".into(),
                head_repo_owner: "x7c1".into(),
                head_repo_name: "delta".into(),
                draft: false,
                url: "https://github.com/x7c1/delta/pull/42".into(),
                updated_at: "2026-06-24T00:00:00Z".into(),
                author_login: "x7c1".into(),
                has_local_clone: true,
            }],
        });
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!({
                "gh_available": true,
                "pull_requests": [{
                    "number": 42,
                    "title": "feat: x",
                    "repo_owner": "x7c1",
                    "repo_name": "delta",
                    "head_ref": "feat/x",
                    "head_repo_owner": "x7c1",
                    "head_repo_name": "delta",
                    "draft": false,
                    "url": "https://github.com/x7c1/delta/pull/42",
                    "updated_at": "2026-06-24T00:00:00Z",
                    "author_login": "x7c1",
                    "has_local_clone": true,
                }],
            }),
        );
    }
}

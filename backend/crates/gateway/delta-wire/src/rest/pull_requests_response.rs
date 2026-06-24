//! Response for `GET /api/prs?lens=…`: the new-session PR tab's per-lens
//! pull-request list, plus a flag telling the UI whether the `gh` CLI
//! could be used at all (so an unauthenticated host can render an inline
//! hint without the endpoint having to 5xx).

use delta_usecase::{PullRequest as DomainPullRequest, PullRequestList};
use serde::Serialize;
use ts_rs::TS;

/// One pull request row in the PR tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PullRequest")]
pub struct WirePullRequest {
    pub number: i64,
    pub title: String,
    /// `<owner>` segment of the base repository (e.g. `x7c1`).
    pub repo_owner: String,
    /// `<name>` segment of the base repository (e.g. `delta`).
    pub repo_name: String,
    /// Branch the PR proposes to merge — typically non-default, which is
    /// why the PR tab defaults the worktree toggle on.
    pub head_ref: String,
    /// `<owner>` segment of the head repository (distinct from
    /// `repo_owner` for a cross-fork PR).
    pub head_repo_owner: String,
    /// `<name>` segment of the head repository.
    pub head_repo_name: String,
    /// True for a draft PR — surfaced as a badge on the row.
    pub draft: bool,
    /// The PR's web URL.
    pub url: String,
    /// ISO-8601 UTC timestamp of the PR's last update.
    pub updated_at: String,
    /// GitHub login of the author.
    pub author_login: String,
    /// True when Delta knows at least one local clone of the PR's base
    /// repository. The UI gates the click → composer pre-fill on this:
    /// false rows are visibly de-emphasised with a "no local clone"
    /// inline hint and the click is silently blocked.
    pub has_local_clone: bool,
}

impl From<DomainPullRequest> for WirePullRequest {
    fn from(pr: DomainPullRequest) -> Self {
        WirePullRequest {
            number: pr.number,
            title: pr.title,
            repo_owner: pr.repo_owner,
            repo_name: pr.repo_name,
            head_ref: pr.head_ref,
            head_repo_owner: pr.head_repo_owner,
            head_repo_name: pr.head_repo_name,
            draft: pr.draft,
            url: pr.url,
            updated_at: pr.updated_at,
            author_login: pr.author_login,
            has_local_clone: pr.has_local_clone,
        }
    }
}

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

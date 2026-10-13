//! One pull request row in the PR tab.

use delta_usecase::PullRequest as DomainPullRequest;
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
    /// why picking the PR starts the session in a worktree on this branch.
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

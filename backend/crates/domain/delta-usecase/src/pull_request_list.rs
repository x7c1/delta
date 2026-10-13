//! What `list_pull_requests` returns: the PRs plus the gh-availability flag.

use crate::PullRequest;

/// What `list_pull_requests` returns: the PRs plus the gh-availability
/// flag.
///
/// `gh_available` is `false` when the `gh` CLI is missing OR
/// `gh auth status` fails. In that case `pull_requests` is empty and the
/// endpoint still returns 200, so the PR tab renders an inline "run
/// `gh auth login`" hint rather than a generic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestList {
    pub gh_available: bool,
    pub pull_requests: Vec<PullRequest>,
}

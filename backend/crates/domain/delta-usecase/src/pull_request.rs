//! Pull-request types for the new-session PR tab.
//!
//! A `PullRequest` is the per-PR row the picker renders, derived from the
//! gateway's PR search. `Lens` picks which of the two `gh` queries to run (the
//! reviewer-non-draft lens, and the author-mine lens, both surfaced on the
//! tab side-by-side). `PullRequestList` is what the use case returns — the
//! PRs themselves plus a flag telling the UI whether `gh` is available at
//! all, so the unauthenticated/uninstalled case can render an inline hint
//! without the endpoint having to 5xx.
//!
//! The `has_local_clone` flag on each PR is derived by the use case (joining
//! the gh result with the Repository tab's clone aggregation): true when at
//! least one local clone of the PR's repository is registered, which is the
//! signal the UI uses to gate the click → pre-fill behaviour.

use std::fmt;

/// Which PR search query backs a PR list.
///
/// `Reviewer` is "open PRs that requested my review and are not drafts" —
/// the inbox lens; `Author` is "open PRs I authored" — useful for picking
/// up your own in-flight work, which includes your own drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestLens {
    /// Open PRs that requested the authenticated user's review and are not
    /// drafts. Backs the "Requested for your review" section.
    Reviewer,
    /// Open PRs authored by the authenticated user, drafts included. Backs
    /// the "Yours" section so a still-draft branch is one click away.
    Author,
}

impl PullRequestLens {
    /// Parse the `lens=` query-string parameter the HTTP endpoint accepts.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "reviewer" => Some(Self::Reviewer),
            "author" => Some(Self::Author),
            _ => None,
        }
    }

    /// The canonical wire form used in the `lens=` query string and the
    /// per-lens cache key.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer",
            Self::Author => "author",
        }
    }
}

impl fmt::Display for PullRequestLens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One pull request row in the PR tab.
///
/// The fields mirror the projection the gateway's PR search asks for, plus
/// `has_local_clone` which the use case derives from the session-history
/// aggregation: true when at least one registered clone of
/// `(repo_owner, repo_name)` exists on disk, so the UI can render the row
/// as clickable (vs. de-emphasised + inline "no local clone" hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    /// The base repository's owner segment (e.g. `x7c1`).
    pub repo_owner: String,
    /// The base repository's name segment (e.g. `delta`).
    pub repo_name: String,
    /// The branch the PR proposes to merge — typically a non-default
    /// branch, which is why the PR tab defaults the worktree toggle on.
    pub head_ref: String,
    /// The head repository's owner segment, distinct from `repo_owner` for
    /// a cross-fork PR.
    pub head_repo_owner: String,
    /// The head repository's name segment, distinct from `repo_name` for
    /// a cross-fork PR.
    pub head_repo_name: String,
    /// True for a draft PR — surfaced as a badge on the row.
    pub draft: bool,
    /// The PR's web URL (used for the row's external-link affordance).
    pub url: String,
    /// ISO-8601 UTC timestamp of the PR's most recent update, for the
    /// relative-time label and the row ordering.
    pub updated_at: String,
    /// GitHub login of the PR author, surfaced as the row byline.
    pub author_login: String,
    /// Whether at least one local clone of the PR's base repository is
    /// registered in Delta's session history. The UI gates the click →
    /// composer pre-fill on this — false rows are visibly de-emphasised
    /// with a "no local clone" inline hint and the click is silently
    /// blocked.
    pub has_local_clone: bool,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_two_lenses_and_rejects_others() {
        assert_eq!(
            PullRequestLens::parse("reviewer"),
            Some(PullRequestLens::Reviewer),
        );
        assert_eq!(
            PullRequestLens::parse("author"),
            Some(PullRequestLens::Author)
        );
        assert!(PullRequestLens::parse("everyone").is_none());
        assert!(PullRequestLens::parse("").is_none());
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for lens in [PullRequestLens::Reviewer, PullRequestLens::Author] {
            assert_eq!(PullRequestLens::parse(lens.as_str()), Some(lens));
        }
    }
}

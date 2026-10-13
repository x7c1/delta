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

#[cfg(doc)]
use crate::PullRequestList;

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
    /// branch, which is why picking the PR starts the session in a worktree
    /// on this branch.
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

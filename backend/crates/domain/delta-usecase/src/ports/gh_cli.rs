//! Driving the `gh` CLI for the PR tab.
//!
//! The PR tab needs two things from `gh`: whether it is even usable on this
//! host (installed and authenticated), and the search results behind each
//! lens. Both go through this port so the gateway owns subprocess concerns
//! and the use case stays a pure orchestration.

use async_trait::async_trait;

use crate::error::Result;
use crate::pull_request::{PullRequest, PullRequestLens};

/// Drives the `gh` CLI for the new-session PR tab.
///
/// Two responsibilities: report whether `gh` is even usable on this host
/// (installed AND authenticated), and run the per-lens `gh search prs`
/// query. The availability check is process-cached by the gateway — once
/// answered it stays answered until the server restarts, so opening the PR
/// tab is cheap on a host where `gh` is missing; the per-lens results are
/// also briefly memoised so toggling between tabs does not re-shell on
/// every focus change.
///
/// Neither method must surface a 5xx to the caller when `gh` is missing
/// or unauthenticated: the use case represents that as
/// `is_authenticated() == false` and an empty list, so the UI can render
/// an inline hint without breaking the whole panel.
#[async_trait]
pub trait GhCli: Send + Sync {
    /// Whether `gh` is installed AND `gh auth status` succeeds.
    ///
    /// A missing binary or any non-zero exit collapses to `false` — both
    /// are surfaced to the UI as "PR tab disabled, run `gh auth login`"
    /// rather than as a server error.
    async fn is_authenticated(&self) -> bool;

    /// Run `gh search prs` for `lens` and return the parsed PR rows.
    ///
    /// `has_local_clone` on the rows is left at its default (`false`) —
    /// the use case fills it in by joining against the registered
    /// repositories. The gateway only owns the gh subprocess + JSON
    /// parsing.
    async fn search_prs(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>>;
}

#[async_trait]
impl GhCli for Box<dyn GhCli> {
    async fn is_authenticated(&self) -> bool {
        (**self).is_authenticated().await
    }

    async fn search_prs(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>> {
        (**self).search_prs(lens).await
    }
}

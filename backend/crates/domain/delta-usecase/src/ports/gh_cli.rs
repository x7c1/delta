//! Driving the `gh` CLI for the PR tab.
//!
//! The PR tab needs three things from `gh`: whether it is even usable on this
//! host (installed and authenticated), the search results behind each lens, and
//! — for a PR whose repository has no local clone yet — the clone itself. All
//! three go through this port so the gateway owns subprocess concerns and the
//! use case stays a pure orchestration.

use async_trait::async_trait;

use crate::error::Result;
use crate::pull_request::{PullRequest, PullRequestLens};

/// Drives the `gh` CLI for the new-session PR tab.
///
/// Three responsibilities: report whether `gh` is even usable on this host
/// (installed AND authenticated), run the per-lens PR search query, and clone
/// a repository the user has no local clone of. The availability check is
/// process-cached by the gateway — once answered it stays answered until the
/// server restarts, so opening the PR tab is cheap on a host where `gh` is
/// missing; the per-lens results are also briefly memoised so toggling between
/// tabs does not re-shell on every focus change.
///
/// Neither *query* method must surface a 5xx to the caller when `gh` is missing
/// or unauthenticated: the use case represents that as
/// `is_authenticated() == false` and an empty list, so the UI can render
/// an inline hint without breaking the whole panel. [`Self::clone_repo`] is the
/// exception — it is a command reached only from an already-available `gh`, so
/// its failure is a real error the user is shown.
#[async_trait]
pub trait GhCli: Send + Sync {
    /// Whether `gh` is installed AND `gh auth status` succeeds.
    ///
    /// A missing binary or any non-zero exit collapses to `false` — both
    /// are surfaced to the UI as "PR tab disabled, run `gh auth login`"
    /// rather than as a server error.
    async fn is_authenticated(&self) -> bool;

    /// Run the PR search for `lens` and return the parsed PR rows.
    ///
    /// `has_local_clone` on the rows is left at its default (`false`) —
    /// the use case fills it in by joining against the registered
    /// repositories. The gateway owns only the gh subprocess and its JSON
    /// parsing.
    async fn search_prs(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>>;

    /// Clone `owner/name` into `destination`, which must not exist yet.
    ///
    /// The caller (the clone use case) owns *where* a clone lands: it passes a
    /// temporary sibling path inside the clone root and renames the result onto
    /// the real destination itself, so this port never has to know about the
    /// half-cloned-directory problem. All it does is run one `gh repo clone`
    /// and report whether it succeeded.
    ///
    /// Unlike the two query methods, a failure here is a genuine
    /// [`crate::Error::Gh`]: the caller only reaches this method after the PR
    /// tab already reported `gh` as available, so a non-zero exit means the
    /// clone really failed (no such repository, no network, no permission) and
    /// the message is shown to the user.
    async fn clone_repo(&self, owner: &str, name: &str, destination: &str) -> Result<()>;
}

#[async_trait]
impl GhCli for Box<dyn GhCli> {
    async fn is_authenticated(&self) -> bool {
        (**self).is_authenticated().await
    }

    async fn search_prs(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>> {
        (**self).search_prs(lens).await
    }

    async fn clone_repo(&self, owner: &str, name: &str, destination: &str) -> Result<()> {
        (**self).clone_repo(owner, name, destination).await
    }
}

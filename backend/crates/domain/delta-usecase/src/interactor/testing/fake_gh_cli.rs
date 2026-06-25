//! In-memory [`GhCli`] fake for the PR-tab use-case tests.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::GhCli;
use crate::pull_request::{PullRequest, PullRequestLens};

/// Scripts both gh-availability and the per-lens search results.
///
/// `authenticated` flips the `gh auth status` answer; `reviewer_prs` /
/// `author_prs` carry the canned `gh search prs` result the matching lens
/// returns. Tests build it directly and pass it to
/// [`crate::Interactor::with_gh_cli`].
#[derive(Default)]
pub(crate) struct FakeGhCli {
    pub(crate) authenticated: bool,
    pub(crate) reviewer_prs: Mutex<Vec<PullRequest>>,
    pub(crate) author_prs: Mutex<Vec<PullRequest>>,
    /// How many times `search_prs` has been called, summed across lenses.
    /// Tests use this to assert the use case's memoisation actually
    /// suppresses repeat shell-outs.
    pub(crate) search_calls: AtomicUsize,
}

impl FakeGhCli {
    /// Build an authenticated fake whose reviewer and author lenses both
    /// answer with the supplied vectors verbatim.
    pub(crate) fn authenticated(
        reviewer_prs: Vec<PullRequest>,
        author_prs: Vec<PullRequest>,
    ) -> Self {
        Self {
            authenticated: true,
            reviewer_prs: Mutex::new(reviewer_prs),
            author_prs: Mutex::new(author_prs),
            search_calls: AtomicUsize::new(0),
        }
    }

    /// Build a fake that reports gh as unavailable (binary missing /
    /// `gh auth status` failed). The lenses should never be queried.
    pub(crate) fn unauthenticated() -> Self {
        Self {
            authenticated: false,
            ..Default::default()
        }
    }

    pub(crate) fn search_calls(&self) -> usize {
        self.search_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GhCli for FakeGhCli {
    async fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    async fn search_prs(&self, lens: PullRequestLens) -> Result<Vec<PullRequest>> {
        self.search_calls.fetch_add(1, Ordering::SeqCst);
        let store = match lens {
            PullRequestLens::Reviewer => &self.reviewer_prs,
            PullRequestLens::Author => &self.author_prs,
        };
        Ok(store.lock().unwrap().clone())
    }
}

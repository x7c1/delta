//! In-memory [`GhCli`] fake for the PR-tab and repository-clone use-case tests.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::error::{Error, Result};
use crate::ports::GhCli;
use crate::pull_request::{PullRequest, PullRequestLens};

/// File the fake writes into every clone it produces, so a test can tell the
/// directory that ended up at the destination apart from a stale one.
pub(crate) const CLONE_MARKER: &str = "cloned-by-fake-gh";

/// One recorded [`GhCli::clone_repo`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloneCall {
    pub(crate) owner: String,
    pub(crate) name: String,
    /// Where the use case asked this clone to land. Always the temporary
    /// sibling directory, never the final destination — asserting that is how a
    /// test pins the "the destination is never half-cloned" rule.
    pub(crate) destination: String,
}

/// Scripts gh-availability, the per-lens search results, and the clone command.
///
/// `authenticated` flips the `gh auth status` answer; `reviewer_prs` /
/// `author_prs` carry the canned PR search result the matching lens
/// returns. Tests build it directly and pass it to
/// [`crate::Interactor::with_gh_cli`].
///
/// The clone side stands in for the `gh repo clone` subprocess: it records every
/// invocation, can be made to block (so a test can hold a job in flight and
/// observe what a second request does), and can be made to fail.
#[derive(Default)]
pub(crate) struct FakeGhCli {
    pub(crate) authenticated: bool,
    pub(crate) reviewer_prs: Mutex<Vec<PullRequest>>,
    pub(crate) author_prs: Mutex<Vec<PullRequest>>,
    /// How many times `search_prs` has been called, summed across lenses.
    /// Tests use this to assert the use case's memoisation actually
    /// suppresses repeat shell-outs.
    pub(crate) search_calls: AtomicUsize,
    /// Every `clone_repo` invocation, in call order.
    clone_calls: Mutex<Vec<CloneCall>>,
    /// When set, `clone_repo` parks here before doing anything, so a test can
    /// keep a job in flight for as long as it needs. Opened by
    /// [`Self::release_clone`].
    ///
    /// A permit-less semaphore rather than a `Notify`, because the open state
    /// has to be *sticky*: closing it releases everyone parked AND everyone who
    /// arrives afterwards, so releasing before a job has reached the gate cannot
    /// strand it behind a signal it missed.
    clone_gate: Option<Arc<Semaphore>>,
    /// When set, `clone_repo` fails with this message instead of writing
    /// anything — the "gh could not clone that" path.
    clone_error: Mutex<Option<String>>,
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
            ..Default::default()
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

    /// Build an authenticated fake with no canned PR rows — for the clone tests,
    /// which never touch a lens.
    pub(crate) fn cloning() -> Self {
        Self {
            authenticated: true,
            ..Default::default()
        }
    }

    /// Make every clone fail with `message` rather than writing anything.
    pub(crate) fn failing_clone(message: &str) -> Self {
        let fake = Self::cloning();
        *fake.clone_error.lock().unwrap() = Some(message.to_owned());
        fake
    }

    /// Hold every clone at the door until [`Self::release_clone`] is called, so
    /// a test can observe the system while a job is genuinely in flight.
    pub(crate) fn blocking_clone() -> Self {
        Self {
            clone_gate: Some(Arc::new(Semaphore::new(0))),
            ..Self::cloning()
        }
    }

    /// Let every parked (and every later) clone through.
    pub(crate) fn release_clone(&self) {
        if let Some(gate) = &self.clone_gate {
            gate.close();
        }
    }

    pub(crate) fn search_calls(&self) -> usize {
        self.search_calls.load(Ordering::SeqCst)
    }

    /// Every recorded clone invocation, in call order.
    pub(crate) fn clone_calls(&self) -> Vec<CloneCall> {
        self.clone_calls.lock().unwrap().clone()
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

    async fn clone_repo(&self, owner: &str, name: &str, destination: &str) -> Result<()> {
        // Recorded before parking, so a test can see that the call arrived even
        // while the job is still held at the gate.
        self.clone_calls.lock().unwrap().push(CloneCall {
            owner: owner.to_owned(),
            name: name.to_owned(),
            destination: destination.to_owned(),
        });

        if let Some(gate) = &self.clone_gate {
            // The only way this resolves is the gate being closed by
            // `release_clone`, which is exactly the signal we are waiting for.
            let _ = gate.acquire().await;
        }

        if let Some(message) = self.clone_error.lock().unwrap().clone() {
            return Err(Error::Gh(message));
        }

        // Stand in for what a real `gh repo clone` leaves behind: a directory at
        // `destination` with content in it.
        tokio::fs::create_dir_all(destination)
            .await
            .map_err(|err| Error::Gh(err.to_string()))?;
        tokio::fs::write(format!("{destination}/{CLONE_MARKER}"), b"cloned\n")
            .await
            .map_err(|err| Error::Gh(err.to_string()))?;
        Ok(())
    }
}

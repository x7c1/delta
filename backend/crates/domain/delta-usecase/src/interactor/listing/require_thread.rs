use delta_model::ThreadId;

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Ensure a thread exists, turning a stale/wrong id into a clean
    /// `ThreadNotFound` instead of an opaque foreign-key error downstream.
    pub(in crate::interactor::listing) async fn require_thread(
        &self,
        thread_id: ThreadId,
    ) -> Result<()> {
        if self.store.thread(thread_id).await?.is_none() {
            return Err(Error::ThreadNotFound(thread_id.value()));
        }
        Ok(())
    }
}

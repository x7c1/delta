use delta_model::ThreadId;

use crate::error::{Error, Result};
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
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

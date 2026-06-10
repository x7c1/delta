use delta_model::SessionId;

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
    /// Ensure a known session is open, returning the pane to dispatch into.
    ///
    /// If it is already open the existing pane is returned; otherwise it is
    /// resumed via [`Self::open_session`] and the freshly-bound pane is returned.
    pub(in crate::interactor::enqueue) async fn ensure_open(
        &self,
        id: &SessionId,
    ) -> Result<String> {
        {
            let registry = self.open_sessions.lock().await;
            if let Some(handle) = registry.handle(id) {
                return Ok(handle.pane.clone());
            }
        }
        // Not open: resume it. `open_session` binds the new pane under the lock.
        self.open_session(id).await?;
        let registry = self.open_sessions.lock().await;
        registry
            .handle(id)
            .map(|h| h.pane.clone())
            .ok_or_else(|| Error::SessionNotFound(id.as_str().to_owned()))
    }
}

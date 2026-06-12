use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Ensure the session is open, returning the pane to dispatch into.
    ///
    /// If it is already open the existing pane is returned; otherwise it is
    /// resumed via [`Self::open_session`] and the freshly-bound pane is
    /// returned.
    pub(in crate::interactor) async fn ensure_open(&mut self) -> Result<String> {
        if let Some(handle) = self.state.handle() {
            return Ok(handle.pane.clone());
        }
        // Not open: resume it. `open_session` binds the new pane.
        self.open_session().await?;
        self.state
            .handle()
            .map(|h| h.pane.clone())
            .ok_or_else(|| Error::SessionNotFound(self.id.as_str().to_owned()))
    }
}

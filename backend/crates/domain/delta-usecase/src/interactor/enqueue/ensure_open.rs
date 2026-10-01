use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Ensure the session is open, returning the pane to dispatch into.
    ///
    /// If it is already open the existing pane is returned; otherwise it is
    /// resumed via [`Self::open_session`] (or a still-running remembered pane
    /// adopted) and the freshly-bound pane is returned.
    ///
    /// Both callers type into the pane that comes back, so a pane whose agent
    /// can no longer deliver hooks to this server is refused with
    /// [`Error::SessionHooksUnreachable`] before anything is typed or written
    /// — whether the session was already open on it or `open_session` just
    /// adopted it (see that variant for why). The session stays open on that
    /// pane; closing it is what lets the next send resume it.
    pub(in crate::interactor) async fn ensure_open(&mut self) -> Result<String> {
        if self.state.handle().is_none() {
            // Not open: resume it. `open_session` binds the new pane.
            self.open_session().await?;
        }
        let handle = self
            .state
            .handle()
            .ok_or_else(|| Error::SessionNotFound(self.id.as_str().to_owned()))?;
        if handle.hooks_unreachable {
            return Err(Error::SessionHooksUnreachable(self.id.as_str().to_owned()));
        }
        Ok(handle.pane.clone())
    }
}

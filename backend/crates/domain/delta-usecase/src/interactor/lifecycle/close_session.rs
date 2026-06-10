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
    /// Close an open session: kill its pane and drop it from the registry.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. Closing a known session that is not open
    /// is a no-op (it has no live pane to tear down), but an *unknown* id is a
    /// clean `SessionNotFound` (404) — the same rejection [`Self::open_session`]
    /// gives — so the browser can tell "already closed" apart from "no such
    /// session" rather than having a stale id silently succeed.
    pub async fn close_session(&self, id: &SessionId) -> Result<()> {
        if self.store.session(id).await?.is_none() {
            return Err(Error::SessionNotFound(id.as_str().to_owned()));
        }
        let handle = {
            let mut registry = self.open_sessions.lock().await;
            registry.remove(id)
        };
        if let Some(handle) = handle {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        Ok(())
    }
}

use delta_model::{Send, SessionId};

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
    /// A session's open (non-terminal) sends — status `queued` or
    /// `dispatched` — oldest first.
    ///
    /// Backs `GET /api/sessions/{id}/sends`, so the browser can render its
    /// send strip from server state instead of mirroring the queue
    /// client-side. A stale or unknown session id is reported as a clean
    /// `SessionNotFound` (404) rather than yielding a silently empty list, so
    /// the browser can tell "nothing pending" apart from "no such session"
    /// (e.g. a spawn that failed and was reaped).
    pub async fn open_sends_for(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        if self.store.session(session_id).await?.is_none() {
            return Err(Error::SessionNotFound(session_id.as_str().to_owned()));
        }
        self.store.open_sends(session_id).await
    }
}

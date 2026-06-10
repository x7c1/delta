use delta_model::{SessionId, Thread};

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
    /// The thread tree for a specific session, ordered by creation.
    ///
    /// A stale or unknown session id is reported as a clean `SessionNotFound`
    /// (404) rather than yielding a silently empty list, so the browser can tell
    /// "no threads yet" apart from "no such session".
    pub async fn threads_for(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        if self.store.session(session_id).await?.is_none() {
            return Err(Error::SessionNotFound(session_id.as_str().to_owned()));
        }
        self.store.list_threads(session_id).await
    }
}

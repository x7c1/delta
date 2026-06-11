use delta_model::{Session, SessionId, ThreadId};

use crate::error::Result;
use crate::ports::{
    NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Insert the session row and emit [`SessionEvent::SessionRegistered`],
    /// returning the session and its `main` thread id.
    ///
    /// Takes the raw identifying fields rather than a specific hook type, because
    /// two hooks register a session — the first `UserPromptSubmit` and
    /// `SessionStart(source=startup)` — and both carry `session_id`, `cwd`, and
    /// `transcript_path`. `register_session` is itself insert-if-absent, so a
    /// second call for an already-registered id is harmless; the event is still
    /// emitted (the browser already invalidates its list idempotently on it).
    pub(in crate::interactor::hooks) async fn register_session_row(
        &self,
        session_id: &SessionId,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(Session, ThreadId)> {
        let (session, main_id) = self
            .store
            .register_session(NewSession {
                id: session_id.clone(),
                cwd: cwd.to_owned(),
                transcript_path: transcript_path.to_owned(),
            })
            .await?;
        events.push(SessionEvent::SessionRegistered {
            session_id: session_id.clone(),
        });
        Ok((session, main_id))
    }
}

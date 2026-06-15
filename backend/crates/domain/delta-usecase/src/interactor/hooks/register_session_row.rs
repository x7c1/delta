use delta_model::{Session, SessionId, ThreadId};

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Upsert the session row and emit [`SessionEvent::SessionRegistered`],
    /// returning the session and its `main` thread id.
    ///
    /// For a Delta launch the row already exists as `spawning` (written when the
    /// id was minted), so this *activates* it — `spawning` → `active`, filling
    /// in the hook-reported transcript path; for an external `claude` the row is
    /// inserted fresh (see [`SessionStore::register_session`]).
    ///
    /// Takes the raw identifying fields rather than a specific hook type, because
    /// two hooks register a session — the first `UserPromptSubmit` and
    /// `SessionStart(source=startup)` — and both carry `session_id`, `cwd`, and
    /// `transcript_path`. `register_session` is idempotent for an already-active
    /// id, so a second call is harmless; the event is still emitted (the browser
    /// already invalidates its list idempotently on it).
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

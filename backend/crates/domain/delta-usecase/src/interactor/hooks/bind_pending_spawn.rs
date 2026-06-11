use delta_model::{Session, SessionId};

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Idempotently bind a fresh spawn to its now-known `session_id` and register
    /// the session, returning the registered session when a bind happened here.
    ///
    /// This is the single binding step shared by the two signals that can first
    /// register a Delta spawn: `SessionStart(source=startup)` and the first
    /// `UserPromptSubmit`. Whichever arrives first does the real work; the other
    /// is a no-op. Concretely, under the registry lock it moves the matching
    /// [`PendingSpawn`] `pending → bound[session_id]`
    /// (via [`OpenSessions::bind_pending_spawn`]); then it registers the session
    /// row (emitting [`SessionEvent::SessionRegistered`]) and, when the spawn
    /// carried a deferred `first_prompt` (a composer-initiated New), writes that
    /// held `pending_send` now that the session id exists — so the first prompt
    /// correlates through the normal FIFO machinery whenever its
    /// `UserPromptSubmit` is processed.
    ///
    /// Returns:
    /// - `Ok(Some(session))` — this call bound a pending spawn and registered it.
    /// - `Ok(None)` — no pending spawn matched `session_id` (already bound by a
    ///   prior call, or the id belongs to an external/unknown session). A no-op:
    ///   the caller decides what to do with an unmatched id.
    ///
    /// [`PendingSpawn`]: crate::open_sessions::PendingSpawn
    /// [`OpenSessions::bind_pending_spawn`]: crate::open_sessions::OpenSessions::bind_pending_spawn
    pub(in crate::interactor::hooks) async fn bind_pending_spawn(
        &self,
        session_id: &SessionId,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Option<Session>> {
        // Take the deferred first prompt with the bind, under the registry lock.
        // A `None` here means no *pending* spawn carries this id — either it was
        // already bound (idempotent re-entry) or it is an external/unknown id.
        let Some(outcome) = self
            .open_sessions
            .lock()
            .await
            .bind_pending_spawn(session_id)
        else {
            return Ok(None);
        };

        let (session, main_id) = self
            .register_session_row(session_id, cwd, transcript_path, events)
            .await?;

        // Write the deferred first send now that the session id is known, so the
        // matching `UserPromptSubmit` finds it and the first prompt correlates
        // through the normal machinery. The text was already delivered into the
        // pane by the spawn's launch-time positional prompt (#61), so this only
        // writes the FIFO head.
        if let Some(text) = outcome.first_prompt {
            self.store
                .enqueue_send(&session.id, main_id, None, &text, None)
                .await?;
        }
        Ok(Some(session))
    }
}

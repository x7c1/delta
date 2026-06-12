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
    /// Idempotently bind a fresh spawn to its now-known `session_id` and
    /// activate its eagerly-created session row, returning the activated
    /// session when a bind happened here.
    ///
    /// This is the single binding step shared by the two signals that can first
    /// contact a Delta spawn: `SessionStart(source=startup)` and the first
    /// `UserPromptSubmit`. Whichever arrives first does the real work; the other
    /// is a no-op. Concretely, under the registry lock it moves the matching
    /// [`PendingSpawn`] `pending → bound[session_id]`
    /// (via [`OpenSessions::bind_pending_spawn`]); then it activates the session
    /// row written eagerly at spawn time — `spawning` → `active`, filling in the
    /// hook-reported transcript path that was unknown when the id was minted —
    /// and emits [`SessionEvent::SessionRegistered`]. Any first prompt's `send`
    /// row was already written at spawn time, so no row writing happens at bind.
    ///
    /// Returns:
    /// - `Ok(Some(session))` — this call bound a pending spawn and activated it.
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
        // Move the pending spawn into the bound map, under the registry lock.
        // `false` means no *pending* spawn carries this id — either it was
        // already bound (idempotent re-entry) or it is an external/unknown id.
        if !self
            .open_sessions
            .lock()
            .await
            .bind_pending_spawn(session_id)
        {
            return Ok(None);
        }

        let (session, _main_id) = self
            .register_session_row(session_id, cwd, transcript_path, events)
            .await?;
        Ok(Some(session))
    }
}

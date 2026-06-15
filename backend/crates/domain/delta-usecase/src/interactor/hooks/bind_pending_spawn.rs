use delta_model::Session;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Idempotently bind this session's fresh spawn and activate its
    /// eagerly-created session row, returning the activated session when a
    /// bind happened here.
    ///
    /// This is the single binding step shared by the two signals that can first
    /// contact a Delta spawn: `SessionStart(source=startup)` and the first
    /// `UserPromptSubmit`. Whichever arrives first does the real work; the other
    /// is a no-op. Concretely, it moves the recorded [`PendingSpawn`] into the
    /// bound pane (via [`SessionRuntime::bind_pending_spawn`]); then it
    /// activates the session row written eagerly at spawn time — `spawning` →
    /// `active`, filling in the hook-reported transcript path that was unknown
    /// when the id was minted — and emits [`SessionEvent::SessionRegistered`].
    /// Any first prompt's `send` row was already written at spawn time, so no
    /// row writing happens at bind.
    ///
    /// Returns:
    /// - `Ok(Some(session))` — this call bound the pending spawn and activated it.
    /// - `Ok(None)` — nothing was pending (already bound by a prior call, or
    ///   the id belongs to an external/unknown session). A no-op: the caller
    ///   decides what to do with an unmatched id.
    ///
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    /// [`SessionRuntime::bind_pending_spawn`]: crate::interactor::session_actor::runtime::SessionRuntime::bind_pending_spawn
    pub(in crate::interactor::hooks) async fn bind_pending_spawn(
        &mut self,
        cwd: &str,
        transcript_path: &str,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Option<Session>> {
        // Move the pending spawn into the bound pane. `false` means nothing is
        // pending — either it was already bound (idempotent re-entry) or this
        // is an external/unknown id.
        if !self.state.bind_pending_spawn() {
            return Ok(None);
        }

        let (session, _main_id) = self
            .core
            .register_session_row(self.id, cwd, transcript_path, events)
            .await?;
        Ok(Some(session))
    }
}

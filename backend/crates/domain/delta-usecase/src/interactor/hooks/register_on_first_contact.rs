use delta_model::Session;

use crate::error::Result;
use crate::ports::{
    SessionEvent, SessionStore, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Register a session on the first `UserPromptSubmit` for its id, binding it
    /// to a fresh spawn when one is waiting.
    ///
    /// Reached only when the session row does not yet exist for this id — so
    /// `SessionStart(source=startup)` has not already bound and registered this
    /// spawn (it shares the same idempotent [`Self::bind_pending_spawn`] step).
    /// Two cases are distinguished by whether the id matches a pending spawn:
    ///
    /// - **Fresh spawn binding**: a [`PendingSpawn`] whose Delta-minted
    ///   `session_id` (pinned via `claude --session-id`) equals the hook's
    ///   `session_id` is bound and registered through [`Self::bind_pending_spawn`]
    ///   — which also writes any held `first_prompt` *before* the caller's
    ///   `match_dispatched_send` runs, so the first prompt correlates through the
    ///   normal FIFO machinery.
    /// - **External claude**: no pending spawn carries this session id, so this
    ///   is a `claude` started outside Delta. The session is registered as a
    ///   known-but-closed data session (no [`OpenHandle`]) and a warning is
    ///   logged, preserving today's external-input behaviour.
    ///
    /// [`PendingSpawn`]: crate::open_sessions::PendingSpawn
    /// [`OpenHandle`]: crate::open_sessions::OpenHandle
    pub(in crate::interactor::hooks) async fn register_on_first_contact(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Session> {
        // Idempotent bind+register shared with `SessionStart(source=startup)`. A
        // matching pending spawn is bound and registered here (writing any
        // held first prompt); `None` means there was no pending spawn for
        // this id, so it is an external claude.
        if let Some(session) = self
            .bind_pending_spawn(&hook.session_id, &hook.cwd, &hook.transcript_path, events)
            .await?
        {
            return Ok(session);
        }

        tracing::warn!(
            session_id = %hook.session_id,
            cwd = %hook.cwd,
            "UserPromptSubmit for an unknown session with no matching pending spawn; \
             registering as an external, closed data session"
        );
        self.register_session_row(&hook.session_id, &hook.cwd, &hook.transcript_path, events)
            .await
            .map(|(s, _)| s)
    }
}

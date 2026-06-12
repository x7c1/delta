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
    /// Register an external session on the first `UserPromptSubmit` for its id.
    ///
    /// Reached only when no pending spawn matched the id (so this is not a
    /// Delta launch — those have an eagerly-created session row and are
    /// activated by [`Self::bind_pending_spawn`]) and no session row exists: a
    /// `claude` started outside Delta. The session is registered as a
    /// known-but-closed data session (no [`OpenHandle`]) and a warning is
    /// logged, preserving today's external-input behaviour.
    ///
    /// [`OpenHandle`]: crate::open_sessions::OpenHandle
    pub(in crate::interactor::hooks) async fn register_on_first_contact(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Session> {
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

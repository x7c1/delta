use crate::error::Result;
use crate::ports::{SessionLifecycle, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Ensure at least one Claude Code session is up, spawning one if absent.
    ///
    /// Idempotent for the still-single server surface: if the registry already
    /// holds an open session (bound or pending) it is reused and
    /// [`SessionLifecycle::Ready`] is returned with no side effects. Otherwise a
    /// fresh session is spawned (see [`Self::new_session`]) and
    /// [`SessionLifecycle::Starting`] is returned.
    ///
    /// This drives only the tmux/process lifecycle. The conversational session
    /// is still registered later by the first `UserPromptSubmit` hook, so a
    /// freshly spawned session has no `Session` row yet — that is expected.
    pub async fn ensure_session(&self) -> Result<SessionLifecycle> {
        {
            let registry = self.open_sessions.lock().await;
            if registry.has_any_live() {
                return Ok(SessionLifecycle::Ready);
            }
        }
        self.new_session().await?;
        Ok(SessionLifecycle::Starting)
    }
}

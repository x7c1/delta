use delta_model::{Session, ThreadId};

use crate::error::Result;
use crate::ports::{
    NewSession, SessionEvent, SessionStore, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
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
    pub(in crate::interactor::hooks) async fn register_session_row(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(Session, ThreadId)> {
        let (session, main_id) = self
            .store
            .register_session(NewSession {
                id: hook.session_id.clone(),
                cwd: hook.cwd.clone(),
                transcript_path: hook.transcript_path.clone(),
            })
            .await?;
        events.push(SessionEvent::SessionRegistered {
            session_id: hook.session_id.clone(),
        });
        Ok((session, main_id))
    }
}

use delta_model::Session;

use crate::error::Result;
use crate::open_sessions::OpenHandle;
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
    /// The first time Claude Code reports a `session_id`, two cases are
    /// distinguished by whether that id matches a pending spawn:
    ///
    /// - **Fresh spawn binding**: a [`PendingSpawn`] whose Delta-minted
    ///   `session_id` (pinned via `claude --session-id`) equals the hook's
    ///   `session_id` is moved `pending → bound[session_id]`. The session row is
    ///   registered (from the hook's `cwd`/`transcript_path`), and if the spawn
    ///   carried a deferred `first_prompt` (a composer-initiated New), the held
    ///   `pending_send` is written *now* — with the now-known session id —
    ///   *before* the caller's `match_pending_send` runs, so the first prompt
    ///   correlates through the normal FIFO machinery.
    /// - **External claude**: no pending spawn carries this session id, so this
    ///   is a `claude` started outside Delta. The session is registered as a
    ///   known-but-closed data session (no [`OpenHandle`]) and a warning is
    ///   logged, preserving today's external-input behaviour.
    ///
    /// [`PendingSpawn`]: crate::open_sessions::PendingSpawn
    pub(in crate::interactor::hooks) async fn register_on_first_contact(
        &self,
        hook: &UserPromptSubmitHook,
        events: &mut Vec<SessionEvent>,
    ) -> Result<Session> {
        // Match a waiting spawn by the Delta-minted session id under the
        // registry lock, taking its deferred first prompt with it.
        let bound = {
            let mut registry = self.open_sessions.lock().await;
            match registry.take_pending_for_session(&hook.session_id) {
                Some(spawn) => {
                    registry.bind(
                        hook.session_id.clone(),
                        OpenHandle {
                            token: spawn.token,
                            pane: spawn.pane,
                            workdir: spawn.workdir,
                        },
                    );
                    spawn.first_prompt
                }
                None => {
                    tracing::warn!(
                        session_id = %hook.session_id,
                        cwd = %hook.cwd,
                        "UserPromptSubmit for an unknown session with no matching pending spawn; \
                         registering as an external, closed data session"
                    );
                    return self
                        .register_session_row(hook, events)
                        .await
                        .map(|(s, _)| s);
                }
            }
        };

        let (session, main_id) = self.register_session_row(hook, events).await?;

        // Write the deferred first send now that the session id is known, so the
        // caller's `match_pending_send` finds it and the first prompt correlates
        // through the normal machinery. The text is sent into the pane up front
        // by the spawn's keystroke dispatch, so this only writes the FIFO head.
        if let Some(text) = bound {
            self.store
                .enqueue_send(&session.id, main_id, None, &text, None)
                .await?;
        }
        Ok(session)
    }
}

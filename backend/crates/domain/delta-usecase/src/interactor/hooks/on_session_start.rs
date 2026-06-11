use crate::error::Result;
use crate::ports::{
    SessionEvent, SessionStartHook, SessionStore, TmuxDriver, Transcript, Workspace,
};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `SessionStart` hook: the session's TUI is ready to accept input.
    ///
    /// This is the event-driven readiness signal that replaces the old fixed
    /// post-launch settle. Behaviour is gated on `source`:
    ///
    /// - **`startup`** — a fresh launch reached its prompt. If a [`PendingSpawn`]
    ///   matches this `session_id`, bind and register it now (the idempotent
    ///   [`Self::bind_pending_spawn`] shared with the first `UserPromptSubmit`),
    ///   so even a prompt-less plain spawn registers immediately instead of
    ///   waiting for a first prompt that may never come. A no-op when no pending
    ///   spawn matches (already bound by the `UserPromptSubmit`, or an external
    ///   id).
    /// - **`resume`** — `claude --resume <id>` finished replaying and is ready.
    ///   Release the held first prompt for that session (see
    ///   [`Self::open_session`]): mark it ready and dispatch the held keystroke on
    ///   the normal `send_line` path now that the cold pane can accept it. A
    ///   no-op when the session is not resuming (already ready, or never resumed
    ///   under Delta).
    /// - **`clear` / `compact`** — fire mid-session on an already-live session
    ///   (the user cleared the context, or it was auto/manually compacted). These
    ///   are not launches, so they must not be treated as a new launch: handled
    ///   as explicit, safe no-ops.
    ///
    /// [`PendingSpawn`]: crate::open_sessions::PendingSpawn
    pub async fn on_session_start(&self, hook: SessionStartHook) -> Result<Vec<SessionEvent>> {
        let mut events = Vec::new();
        match hook.source.as_str() {
            SessionStartHook::SOURCE_STARTUP => {
                match self
                    .bind_pending_spawn(
                        &hook.session_id,
                        &hook.cwd,
                        &hook.transcript_path,
                        &mut events,
                    )
                    .await?
                {
                    Some(_) => {
                        tracing::info!(
                            session_id = %hook.session_id,
                            "SessionStart(startup): bound and registered a pending spawn"
                        );
                    }
                    None => {
                        tracing::debug!(
                            session_id = %hook.session_id,
                            "SessionStart(startup): no matching pending spawn (already bound \
                             or external); no-op"
                        );
                    }
                }
            }
            SessionStartHook::SOURCE_RESUME => {
                self.release_resumed_first_prompt(&hook.session_id).await?;
            }
            other => {
                // clear/compact (and any unknown future source) fire on an
                // already-live session; they are not a launch, so binding and
                // readiness handling stay out of it.
                tracing::debug!(
                    session_id = %hook.session_id,
                    source = %other,
                    "SessionStart for a mid-session source; no launch/readiness handling"
                );
            }
        }
        Ok(events)
    }
}

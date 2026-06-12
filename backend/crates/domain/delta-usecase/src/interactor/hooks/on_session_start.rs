use crate::error::Result;
use crate::ports::{
    SessionEvent, SessionStartHook, SessionStore, TmuxDriver, Transcript, Workspace,
};
use crate::interactor::InteractorCore;

impl<T, X, S, W> InteractorCore<T, X, S, W>
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
    ///   Mark the session ready (stamp its `ready_at`) and return immediately;
    ///   the held first prompt is **not** dispatched here. This hook blocks
    ///   `claude` until the handler returns, so a keystroke typed now would land
    ///   while `claude` is still inside the hook and not accepting input, and be
    ///   silently lost. Instead the held prompt is dispatched a beat later by
    ///   [`Self::dispatch_ready_resumes`] on the background tick, after the hook
    ///   has returned and `claude` is input-ready (see [`Self::open_session`]). A
    ///   no-op when the session is not resuming (already dispatched, or never
    ///   resumed under Delta).
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
                // Only mark ready and return immediately — do NOT dispatch the
                // held prompt here. This hook blocks `claude` until the handler
                // returns; the held keystroke is dispatched later by
                // `dispatch_ready_resumes` on the background tick, after `claude`
                // has left the hook and is input-ready.
                let marked = self
                    .open_sessions
                    .lock()
                    .await
                    .mark_resume_ready_at(&hook.session_id, std::time::Instant::now());
                if marked {
                    tracing::info!(
                        session_id = %hook.session_id,
                        "SessionStart(resume): marked resume ready; the held first prompt \
                         dispatches on the next background tick once it settles"
                    );
                } else {
                    tracing::debug!(
                        session_id = %hook.session_id,
                        "SessionStart(resume): session not resuming (already dispatched or not \
                         Delta-resumed); no-op"
                    );
                }
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

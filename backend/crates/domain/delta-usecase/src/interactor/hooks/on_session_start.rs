use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStartHook, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `SessionStart` hook: the session's TUI is ready to accept input.
    ///
    /// This is the event-driven readiness signal that replaces the old fixed
    /// post-launch settle. Behaviour is gated on `source`:
    ///
    /// - **`startup`** — a fresh launch reached its prompt. If a [`PendingSpawn`]
    ///   is recorded for this session, bind and register it now (the idempotent
    ///   [`Self::bind_pending_spawn`] shared with the first `UserPromptSubmit`),
    ///   so even a prompt-less plain spawn registers immediately instead of
    ///   waiting for a first prompt that may never come. A no-op when nothing is
    ///   pending (already bound by the `UserPromptSubmit`, or an external id).
    /// - **`resume`** — `claude --resume <id>` finished replaying and is ready.
    ///   Mark the session ready (stamp its `ready_at`) and return immediately;
    ///   the held first prompt is **not** dispatched here. This hook blocks
    ///   `claude` until the handler returns, so a keystroke typed now would land
    ///   while `claude` is still inside the hook and not accepting input, and be
    ///   silently lost. Instead the held prompt is dispatched a beat later by
    ///   the resume tick, after the hook has returned and `claude` is
    ///   input-ready (see [`Self::open_session`]). A no-op when the session is
    ///   not resuming (already dispatched, or never resumed under Delta).
    /// - **`compact`** — fires mid-session on an already-live session once
    ///   Claude Code finishes auto- or manually compacting it. Not a launch
    ///   (so binding/readiness stays out of it), but the compaction routine
    ///   may have swallowed a prompt the user keyed in at the same moment:
    ///   re-type any `Dispatched` `OutstandingSend` so the user's intent is
    ///   preserved. Idempotent with the ingestion-time
    ///   `Effect::AutoCompactFinished` path via the
    ///   `try_claim_auto_compact_redispatch` debounce on `SessionRuntime`.
    /// - **`clear`** — fires mid-session when the user deliberately wipes the
    ///   context. A clear is an intentional reset, so outstanding sends are
    ///   left alone: resurrecting them would invert intent.
    ///
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    pub(in crate::interactor) async fn on_session_start(
        &mut self,
        hook: SessionStartHook,
    ) -> Result<Vec<SessionEvent>> {
        let mut events = Vec::new();
        match hook.source.as_str() {
            SessionStartHook::SOURCE_STARTUP => {
                match self
                    .bind_pending_spawn(&hook.cwd, &hook.transcript_path, &mut events)
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
                // returns; the held keystroke is dispatched later by the resume
                // tick, after `claude` has left the hook and is input-ready.
                let marked = self.state.mark_resume_ready_at(std::time::Instant::now());
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
            SessionStartHook::SOURCE_COMPACT => {
                // Auto/manual `/compact` finished — re-type any send stuck
                // behind the swallowed echo. The debounce inside the
                // helper deduplicates against the ingestion-time
                // `Effect::AutoCompactFinished` path.
                self.try_redispatch_after_compact("SessionStart(compact)")
                    .await?;
            }
            SessionStartHook::SOURCE_CLEAR => {
                // A clear is a deliberate context wipe; resurrecting prior
                // sends would invert intent. Treat as a safe no-op.
                tracing::debug!(
                    session_id = %hook.session_id,
                    "SessionStart(clear): mid-session reset; no re-dispatch"
                );
            }
            other => {
                // Any unknown future source: not a launch, not a known
                // mid-session reset. Logged so a new shape surfaces in the
                // logs instead of silently doing nothing surprising.
                tracing::debug!(
                    session_id = %hook.session_id,
                    source = %other,
                    "SessionStart for an unrecognized source; no launch/readiness handling"
                );
            }
        }
        Ok(events)
    }
}

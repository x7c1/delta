use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{
    GitWorktree, SessionEndHook, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `SessionEnd` hook: catch a launch that died before it became
    /// ready, otherwise treat it as a normal end.
    ///
    /// `SessionEnd` is the precise early failure signal that complements the
    /// watchdog deadline. Three cases are distinguished:
    ///
    /// - **Failed fresh launch**: an *unbound* pending spawn is still recorded,
    ///   so `claude` ended before its first `UserPromptSubmit`/`SessionStart`
    ///   ever bound it. This is the silent-stall case the watchdog exists for,
    ///   caught here immediately instead of at the full deadline. The spawn is
    ///   removed, its pane torn down best-effort, and a
    ///   [`SessionEvent::SpawnFailed`] emitted so the browser clears the
    ///   optimistic pending chip.
    /// - **Failed resume**: the session is resumed but not yet ready (its first
    ///   prompt is still held awaiting `SessionStart(source=resume)`). The
    ///   resume ended before readiness, so it is the same failure: the resuming
    ///   entry is dropped, its pane torn down, any held prompt cancelled, and a
    ///   `SpawnFailed` emitted. A resume records no pending spawn, so without
    ///   this it would be misread as a normal end.
    /// - **Normal end**: neither — the session is already-ready/bound, or
    ///   unknown. The launch succeeded and is simply ending; this handler does
    ///   **not** touch close/teardown semantics (owned by `close_session`), it
    ///   just logs and returns cleanly.
    ///
    /// Failure detection is limited to the not-yet-ready cases, so this hook can
    /// never tear down a healthy, already-ready session.
    pub(in crate::interactor) async fn on_session_end(
        &mut self,
        hook: SessionEndHook,
    ) -> Result<Vec<SessionEvent>> {
        // Both failure candidates live on this actor's state; the tmux
        // teardown runs after they are taken, mirroring the reaper.
        let spawn = self.state.take_unbound_pending();
        let resuming = self.state.take_resuming();

        if let Some(spawn) = spawn {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %hook.session_id,
                reason = hook.reason.as_deref().unwrap_or("<none>"),
                "SessionEnd for a still-unbound spawn; treating it as a failed launch \
                 and reporting SpawnFailed"
            );
            self.kill_pane_best_effort(spawn.token.as_str()).await;
            // The spawn never bound, so its eagerly-created `spawning` row (and
            // children, by cascade) is deleted — same cleanup as the watchdog.
            // Its turn entry (set when the first prompt's send was enqueued) is
            // dropped with it.
            self.state.forget_turn();
            // BEFORE the cleanup, which deletes the rows this reads.
            let unsent = self.undelivered_sends(&hook.session_id).await;
            self.clean_up_failed_spawn_row(&hook.session_id).await?;
            return Ok(vec![SessionEvent::SpawnFailed {
                session_id: hook.session_id,
                pane_token: Some(spawn.token.as_str().to_owned()),
                // The hook only reports that the launch ended, never why it
                // never bound, so there is no cause to pass on here.
                reason: None,
                unsent,
            }]);
        }

        if let Some(resuming) = resuming {
            tracing::warn!(
                token = %resuming.token.as_str(),
                session_id = %hook.session_id,
                reason = hook.reason.as_deref().unwrap_or("<none>"),
                "SessionEnd for a resume that ended before becoming ready; \
                 treating it as a failed resume and reporting SpawnFailed"
            );
            self.kill_pane_best_effort(resuming.token.as_str()).await;
            // The session's pane is gone: feed `Close` into the turn machine,
            // which cancels the held first prompt's outstanding send (if any)
            // so its row does not shadow correlation on a later re-resume.
            let _ = self.apply_turn_input(crate::turn::TurnInput::Close).await;
            return Ok(vec![SessionEvent::SpawnFailed {
                session_id: hook.session_id,
                pane_token: Some(resuming.token.as_str().to_owned()),
                reason: None,
                // A failed resume keeps its session row and its send rows: the
                // `Close` above requeued the held prompt rather than deleting
                // it, so there is nothing to hand back to the composer.
                unsent: Vec::new(),
            }]);
        }

        // Normal end (or an unrelated id): the session was ready/bound or unknown.
        // Leave close/teardown semantics alone, but the `claude` process is
        // gone, so whatever turn state it had can no longer progress — feed
        // `Close` so the turn machine does not hold a phantom in-flight turn
        // (a no-op for an idle/unknown session).
        tracing::info!(
            session_id = %hook.session_id,
            reason = hook.reason.as_deref().unwrap_or("<none>"),
            "SessionEnd for a ready/unknown session; normal end, no failure handling"
        );
        self.apply_turn_input(crate::turn::TurnInput::Close).await?;
        // The `claude` process is gone, so no more of this session's transcript
        // is ingested — a lingering BACKGROUND subagent's completion
        // `<task-notification>` can never be folded to clear its indicator. The
        // `Close` above already swept the foreground entries; sweep whatever
        // background entries survive so their indicators do not stick forever,
        // returning a `SubagentFinished` per entry for the caller to broadcast.
        self.sweep_running_subagents_on_process_gone().await
    }
}

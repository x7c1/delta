use crate::error::Result;
use crate::ports::{
    SessionEndHook, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace,
};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `SessionEnd` hook: catch a launch that died before it became
    /// ready, otherwise treat it as a normal end.
    ///
    /// `SessionEnd` is the precise early failure signal that complements the
    /// watchdog deadline. Three cases are distinguished:
    ///
    /// - **Failed fresh launch**: the id still names an *unbound* pending spawn,
    ///   so `claude` ended before its first `UserPromptSubmit`/`SessionStart`
    ///   ever bound it. This is the silent-stall case the watchdog exists for,
    ///   caught here immediately instead of at the full deadline. The spawn is
    ///   removed, its pane torn down best-effort, and a
    ///   [`SessionEvent::SpawnFailed`] emitted so the browser clears the
    ///   optimistic pending chip.
    /// - **Failed resume**: the id names a session that is resumed but not yet
    ///   ready (its first prompt is still held awaiting
    ///   `SessionStart(source=resume)`). The resume ended before readiness, so it
    ///   is the same failure: the resuming entry is dropped, its pane torn down,
    ///   any held prompt cancelled, and a `SpawnFailed` emitted. A resume creates
    ///   no `PendingSpawn`, so without this it would be misread as a normal end.
    /// - **Normal end**: neither — the id is an already-ready/bound session, or
    ///   unknown. The launch succeeded and is simply ending; this handler does
    ///   **not** touch close/teardown semantics (owned by `close_session` and the
    ///   registry), it just logs and returns cleanly.
    ///
    /// Failure detection is limited to the not-yet-ready cases, so this hook can
    /// never tear down a healthy, already-ready session.
    pub async fn on_session_end(&self, hook: SessionEndHook) -> Result<Vec<SessionEvent>> {
        // Resolve both failure candidates under one registry lock; the tmux
        // teardown runs after the lock is dropped, mirroring the reaper.
        let (spawn, resuming) = {
            let mut registry = self.open_sessions.lock().await;
            (
                registry.take_unbound_pending_for_session(&hook.session_id),
                registry.take_resuming(&hook.session_id),
            )
        };

        if let Some(spawn) = spawn {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %hook.session_id,
                reason = hook.reason.as_deref().unwrap_or("<none>"),
                "SessionEnd for a still-unbound spawn; treating it as a failed launch \
                 and reporting SpawnFailed"
            );
            self.kill_pane_best_effort(spawn.token.as_str()).await;
            return Ok(vec![SessionEvent::SpawnFailed {
                session_id: hook.session_id,
                pane_token: spawn.token.as_str().to_owned(),
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
            // Cancel the held first prompt (if any) so its row does not block the
            // FIFO on a later re-resume.
            if resuming.held_prompt.is_some() {
                if let Some(head) = self.store.head_pending_send(&hook.session_id).await? {
                    let _ = self.store.cancel_send(head.id).await;
                }
                let _ = self.store.set_turn_active(&hook.session_id, false).await;
            }
            return Ok(vec![SessionEvent::SpawnFailed {
                session_id: hook.session_id,
                pane_token: resuming.token.as_str().to_owned(),
            }]);
        }

        // Normal end (or an unrelated id): the session was ready/bound or unknown.
        // Leave close/teardown semantics alone.
        tracing::info!(
            session_id = %hook.session_id,
            reason = hook.reason.as_deref().unwrap_or("<none>"),
            "SessionEnd for a ready/unknown session; normal end, no failure handling"
        );
        Ok(Vec::new())
    }
}

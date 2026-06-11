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
    /// Handle a `SessionEnd` hook: catch a launch that died before it ever
    /// registered, otherwise treat it as a normal end.
    ///
    /// `SessionEnd` is the precise early failure signal that complements the
    /// watchdog deadline. Two cases are distinguished by whether the hook's
    /// session id still names an *unbound* pending spawn:
    ///
    /// - **Failed launch**: the id matches a spawn that is still in `pending`,
    ///   so `claude` ended before its first `UserPromptSubmit` ever bound it.
    ///   This is the silent-stall case the watchdog exists for, caught here
    ///   immediately instead of at the full deadline. The spawn is removed, its
    ///   tmux pane is torn down best-effort (it has usually already exited), and
    ///   a [`SessionEvent::SpawnFailed`] is emitted so the browser can clear the
    ///   optimistic pending chip.
    /// - **Normal end**: no pending spawn carries this id — it is an
    ///   already-bound/known session (or an unrelated id), so the launch
    ///   succeeded and is simply ending. This handler deliberately does **not**
    ///   touch close/teardown semantics for that case (those are owned by
    ///   `close_session` and the registry); it just logs and returns cleanly.
    ///
    /// Failure detection is intentionally limited to the unbound-spawn case so
    /// this hook can never tear down a healthy session.
    pub async fn on_session_end(&self, hook: SessionEndHook) -> Result<Vec<SessionEvent>> {
        // Take the spawn under the registry lock; the tmux teardown runs after
        // the lock is dropped, mirroring the reaper.
        let spawn = self
            .open_sessions
            .lock()
            .await
            .take_unbound_pending_for_session(&hook.session_id);

        let Some(spawn) = spawn else {
            // Normal end (or an unrelated id): the session was bound or unknown,
            // not a still-pending spawn. Leave close/teardown semantics alone.
            tracing::info!(
                session_id = %hook.session_id,
                reason = hook.reason.as_deref().unwrap_or("<none>"),
                "SessionEnd for a bound/unknown session; normal end, no failure handling"
            );
            return Ok(Vec::new());
        };

        tracing::warn!(
            token = %spawn.token.as_str(),
            session_id = %hook.session_id,
            reason = hook.reason.as_deref().unwrap_or("<none>"),
            "SessionEnd for a still-unbound spawn; treating it as a failed launch \
             and reporting SpawnFailed"
        );

        // Best-effort teardown: a crashed/exited launch usually has no live tmux
        // session left, so guard the kill with `has_session` and never let a
        // teardown error suppress the failure report.
        match self.tmux.has_session(spawn.token.as_str()).await {
            Ok(true) => {
                if let Err(err) = self.tmux.kill_session(spawn.token.as_str()).await {
                    tracing::warn!(
                        token = %spawn.token.as_str(),
                        error = %err,
                        "failed to kill the failed spawn's pane (continuing)"
                    );
                }
            }
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    token = %spawn.token.as_str(),
                    error = %err,
                    "failed to probe the failed spawn's pane (continuing)"
                );
            }
        }

        Ok(vec![SessionEvent::SpawnFailed {
            session_id: hook.session_id,
            pane_token: spawn.token.as_str().to_owned(),
        }])
    }
}

use std::time::Instant;

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Reap spawns that never bound before their deadline (the watchdog sweep).
    ///
    /// A fresh spawn is fire-and-forget: `claude` is launched in a tmux pane and
    /// the only thing that registers/binds it is its first `UserPromptSubmit`
    /// hook. If `claude` crashes, exits, or hangs on auth before that hook ever
    /// fires, nothing would otherwise time the dangling spawn out — the pane
    /// lingers and the UI is stuck "pending" forever with no error. This sweep
    /// is the backstop: it removes every unbound spawn whose
    /// [`PENDING_SPAWN_DEADLINE`] has elapsed and, for each, kills its tmux pane
    /// (best-effort, guarded by `has_session`) and produces a
    /// [`SessionEvent::SpawnFailed`] so the browser can surface the failure and
    /// clear the optimistic pending chip.
    ///
    /// `now` is injected (rather than read here) so the watchdog is deterministic
    /// under test. The reap mirrors [`Self::poll_transcript`]: the usecase
    /// returns the events to broadcast and stays free of `tokio::spawn` — the
    /// server owns the periodic tick that calls this and broadcasts the result.
    ///
    /// [`PENDING_SPAWN_DEADLINE`]: crate::open_sessions::PENDING_SPAWN_DEADLINE
    pub async fn reap_stale_spawns(&self, now: Instant) -> Result<Vec<SessionEvent>> {
        // Take the registry lock only long enough to drain the stale spawns; the
        // tmux teardown below runs without the lock so it cannot serialize the
        // hooks or the PTY bridge against per-pane I/O.
        let stale = self.open_sessions.lock().await.drain_stale_pending(now);
        if stale.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::with_capacity(stale.len());
        for spawn in stale {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %spawn.session_id,
                "reaping a spawn that never bound before its deadline; \
                 killing its pane and reporting SpawnFailed"
            );
            // Best-effort teardown: the pane may already be gone (a crashed
            // launch tears its own tmux session down), so guard the kill with
            // `has_session` and never let a teardown error mask the failure
            // report — the spawn is already removed from the registry, so the
            // event must still be emitted regardless.
            match self.tmux.has_session(spawn.token.as_str()).await {
                Ok(true) => {
                    if let Err(err) = self.tmux.kill_session(spawn.token.as_str()).await {
                        tracing::warn!(
                            token = %spawn.token.as_str(),
                            error = %err,
                            "failed to kill the reaped spawn's pane (continuing)"
                        );
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(
                        token = %spawn.token.as_str(),
                        error = %err,
                        "failed to probe the reaped spawn's pane (continuing)"
                    );
                }
            }
            events.push(SessionEvent::SpawnFailed {
                session_id: spawn.session_id,
                pane_token: spawn.token.as_str().to_owned(),
            });
        }
        Ok(events)
    }
}

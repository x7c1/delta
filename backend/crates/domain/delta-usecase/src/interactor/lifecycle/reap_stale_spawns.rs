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
    /// Reap launches that never became ready before their deadline (the watchdog
    /// sweep), covering both fresh spawns and resumed sessions.
    ///
    /// Two fire-and-forget launch shapes can stall the UI on "pending" forever:
    ///
    /// - **Fresh spawn**: `claude` is launched in a tmux pane and the only thing
    ///   that registers/binds it is its first `UserPromptSubmit` (or
    ///   `SessionStart`) hook. If it crashes, exits, or hangs on auth before that
    ///   hook fires, nothing else would time the dangling spawn out. This sweep
    ///   removes every unbound spawn whose [`PENDING_SPAWN_DEADLINE`] has elapsed.
    /// - **Resumed session**: `claude --resume <id>` binds the pane immediately
    ///   but the first prompt is held until `SessionStart(source=resume)` signals
    ///   readiness. A resume that never becomes ready (the resume crashes/hangs,
    ///   or transcript replay fails after the existence gate) leaves that held
    ///   prompt parked forever — and a resume creates no `PendingSpawn`, so the
    ///   spawn sweep above does not cover it. This sweep removes every resuming
    ///   session whose [`RESUME_READY_DEADLINE`] has elapsed, releasing/cancelling
    ///   its held first prompt.
    ///
    /// For each stale launch it kills the tmux pane (best-effort, guarded by
    /// `has_session`) and produces a [`SessionEvent::SpawnFailed`] so the browser
    /// can surface the failure and clear the optimistic pending chip. The same
    /// `SpawnFailed` shape is reused for both: it already carries the
    /// `session_id` + `pane_token` the browser needs, and a resume failure is the
    /// same "this launch never came up" outcome from the UI's point of view, so a
    /// sibling event would add a wire variant without adding information.
    ///
    /// `now` is injected (rather than read here) so the watchdog is deterministic
    /// under test. The reap mirrors [`Self::poll_transcript`]: the usecase
    /// returns the events to broadcast and stays free of `tokio::spawn` — the
    /// server owns the periodic tick that calls this and broadcasts the result.
    ///
    /// [`PENDING_SPAWN_DEADLINE`]: crate::open_sessions::PENDING_SPAWN_DEADLINE
    /// [`RESUME_READY_DEADLINE`]: crate::open_sessions::RESUME_READY_DEADLINE
    pub async fn reap_stale_spawns(&self, now: Instant) -> Result<Vec<SessionEvent>> {
        // Take the registry lock only long enough to drain the stale launches;
        // the tmux teardown below runs without the lock so it cannot serialize
        // the hooks or the PTY bridge against per-pane I/O.
        let (stale_spawns, stale_resumes) = {
            let mut registry = self.open_sessions.lock().await;
            (
                registry.drain_stale_pending(now, self.launch.pending_spawn_deadline),
                registry.drain_stale_resuming(now, self.launch.resume_ready_deadline),
            )
        };
        if stale_spawns.is_empty() && stale_resumes.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::with_capacity(stale_spawns.len() + stale_resumes.len());
        for spawn in stale_spawns {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %spawn.session_id,
                "reaping a spawn that never bound before its deadline; \
                 killing its pane and reporting SpawnFailed"
            );
            self.kill_pane_best_effort(spawn.token.as_str()).await;
            events.push(SessionEvent::SpawnFailed {
                session_id: spawn.session_id,
                pane_token: spawn.token.as_str().to_owned(),
            });
        }
        for (session_id, resuming) in stale_resumes {
            tracing::warn!(
                token = %resuming.token.as_str(),
                session_id = %session_id,
                had_held_prompt = resuming.held_prompt.is_some(),
                "reaping a resume that never became ready before its deadline; \
                 killing its pane, cancelling any held prompt, reporting SpawnFailed"
            );
            self.kill_pane_best_effort(resuming.token.as_str()).await;
            // Cancel the held first prompt's pending row (if a send was waiting on
            // readiness) so it does not block the FIFO when the session is later
            // resumed again.
            if resuming.held_prompt.is_some() {
                if let Some(head) = self.store.head_pending_send(&session_id).await? {
                    let _ = self.store.cancel_send(head.id).await;
                }
                let _ = self.store.set_turn_active(&session_id, false).await;
            }
            events.push(SessionEvent::SpawnFailed {
                session_id,
                pane_token: resuming.token.as_str().to_owned(),
            });
        }
        Ok(events)
    }

    /// Best-effort pane teardown shared by the watchdog sweeps and the
    /// `SessionEnd` failure path: probe with `has_session` and kill if present,
    /// never letting a teardown error mask the failure report (the launch is
    /// already removed from the registry, so the failure event must still fire).
    pub(in crate::interactor) async fn kill_pane_best_effort(&self, token: &str) {
        match self.tmux.has_session(token).await {
            Ok(true) => {
                if let Err(err) = self.tmux.kill_session(token).await {
                    tracing::warn!(
                        token = %token,
                        error = %err,
                        "failed to kill the failed launch's pane (continuing)"
                    );
                }
            }
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    token = %token,
                    error = %err,
                    "failed to probe the failed launch's pane (continuing)"
                );
            }
        }
    }
}

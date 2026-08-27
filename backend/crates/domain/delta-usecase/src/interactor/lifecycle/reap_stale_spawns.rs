use std::time::Instant;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, UnsentSend, Workspace,
};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Reap this session's launch if it never became ready before its deadline
    /// (the watchdog sweep), covering both a fresh spawn and a resume.
    ///
    /// Two fire-and-forget launch shapes can stall the UI on "pending" forever:
    ///
    /// - **Fresh spawn**: `claude` is launched in a tmux pane and the only thing
    ///   that registers/binds it is its first `UserPromptSubmit` (or
    ///   `SessionStart`) hook. If it crashes, exits, or hangs on auth before that
    ///   hook fires, nothing else would time the dangling spawn out. The sweep
    ///   removes an unbound spawn whose deadline has elapsed.
    /// - **Resumed session**: `claude --resume <id>` binds the pane immediately
    ///   but the first prompt is held until `SessionStart(source=resume)` signals
    ///   readiness. A resume that never becomes ready (the resume crashes/hangs,
    ///   or transcript replay fails after the existence gate) leaves that held
    ///   prompt parked forever — and a resume records no pending spawn, so the
    ///   spawn sweep above does not cover it. The sweep removes a resuming entry
    ///   whose readiness deadline has elapsed, cancelling its held first prompt.
    ///
    /// A third shape — a session that has only been *accepted*, with its launch
    /// preparation still running — is deliberately outside this sweep: it holds
    /// a `LaunchingSpawn`, not a pending one, so neither drain sees it. It has
    /// no pane to kill, and its bind deadline only starts when the preparation
    /// checks in (`LaunchPrepared`) a beat before the pane is created; a slow
    /// `git fetch` must not eat that deadline. Its backstop is the launch task's
    /// own [`LAUNCH_PREP_DEADLINE`], after which the launch fails itself and
    /// reports the same `SpawnFailed` (with a `reason`).
    ///
    /// An **adapter-backed** (Codex) session is only ever that third shape while
    /// it is starting: its launch is accepted and deferred exactly like a
    /// Claude one, but the bind is the launch's own last step rather than
    /// something a hook has to deliver afterwards, so it never becomes a pending
    /// spawn and `pending_spawn_deadline` never applies to it. The launch
    /// preparation deadline — which covers the worktree build, the `connect` and
    /// the `thread/start` together — is its only watchdog, and it needs no
    /// other: an adapter launch that hangs hangs *inside* that window.
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
    /// under test. The usecase returns the events to broadcast — the server owns
    /// the periodic tick that fans this out and broadcasts the result.
    ///
    /// [`LAUNCH_PREP_DEADLINE`]: crate::launch_config::LAUNCH_PREP_DEADLINE
    pub(in crate::interactor) async fn reap_stale_launch(
        &mut self,
        now: Instant,
    ) -> Result<Vec<SessionEvent>> {
        let stale_spawn = self
            .state
            .take_stale_pending(now, self.launch.pending_spawn_deadline);
        let stale_resume = self
            .state
            .take_stale_resuming(now, self.launch.resume_ready_deadline);

        let mut events = Vec::new();
        if let Some(spawn) = stale_spawn {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %self.id,
                "reaping a spawn that never bound before its deadline; \
                 killing its pane and reporting SpawnFailed"
            );
            self.kill_pane_best_effort(spawn.token.as_str()).await;
            // The row (and any first send, by cascade) is deleted; drop the
            // turn entry with it.
            self.state.forget_turn();
            let session_id = self.id.clone();
            // BEFORE the cleanup, which deletes the rows this reads.
            let unsent = self.undelivered_sends(&session_id).await;
            self.clean_up_failed_spawn_row(&session_id).await?;
            events.push(SessionEvent::SpawnFailed {
                session_id,
                pane_token: Some(spawn.token.as_str().to_owned()),
                // The watchdog observes silence, not a cause: nothing said why
                // the launch never bound.
                reason: None,
                unsent,
            });
        }
        if let Some(resuming) = stale_resume {
            tracing::warn!(
                token = %resuming.token.as_str(),
                session_id = %self.id,
                had_held_prompt = resuming.held_prompt.is_some(),
                "reaping a resume that never became ready before its deadline; \
                 killing its pane, cancelling any held prompt, reporting SpawnFailed"
            );
            self.kill_pane_best_effort(resuming.token.as_str()).await;
            // The session's pane is gone: feed `Close` into the turn machine,
            // which cancels the held first prompt's outstanding send (if any)
            // so its row does not shadow correlation when the session is later
            // resumed again.
            let _ = self.apply_turn_input(crate::turn::TurnInput::Close).await;
            events.push(SessionEvent::SpawnFailed {
                session_id: self.id.clone(),
                pane_token: Some(resuming.token.as_str().to_owned()),
                reason: None,
                // A resume keeps its session row and every send row with it —
                // the `Close` above requeued the held prompt rather than
                // dropping it — so nothing is about to be deleted and there is
                // no text to hand back.
                unsent: Vec::new(),
            });
        }
        Ok(events)
    }
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The sends a failed launch accepted but never delivered to an agent,
    /// oldest first — the text the browser puts back in its composer.
    ///
    /// A spawn that never bound reached no agent at all, so *every* open send
    /// of the session qualifies: the first prompt (`dispatched` for a Claude
    /// spawn, whose prompt rides the launch command line; `queued` for an
    /// adapter-backed one, whose prompt waits for the provider thread) and each
    /// send accepted as `queued` while the launch was still running.
    /// [`SessionStore::open_sends`] is exactly that set, in id order.
    ///
    /// Must be called BEFORE [`Self::clean_up_failed_spawn_row`]: the rows
    /// cascade away with the session, and this frame is the last place their
    /// text exists.
    ///
    /// A read failure is logged and reported as "nothing outstanding" rather
    /// than propagated: the browser is waiting on a session that will never
    /// come up, and losing the failure report over a failed query would be the
    /// worse outcome.
    pub(in crate::interactor) async fn undelivered_sends(
        &self,
        session_id: &delta_model::SessionId,
    ) -> Vec<UnsentSend> {
        match self.store.open_sends(session_id).await {
            Ok(sends) => sends
                .into_iter()
                .map(|send| UnsentSend {
                    send_id: send.id,
                    text: send.text,
                })
                .collect(),
            Err(err) => {
                tracing::error!(
                    session_id = %session_id,
                    error = %err,
                    "failed to read the undelivered sends of a failed launch; \
                     reporting the failure without them (their text is lost)"
                );
                Vec::new()
            }
        }
    }

    /// Clean up the eagerly-created session row of a spawn that never bound.
    ///
    /// The row was INSERTed (status `spawning`) when the id was minted, before
    /// `claude` launched. A spawn that never bound ingested nothing, so the row
    /// — and its main thread plus every `send` row, removed by cascade — is
    /// deleted outright rather than kept as a `failed` tombstone. The user's
    /// text is not lost with them: the composer's Retry/Dismiss chip holds the
    /// FIRST prompt browser-side, and [`Self::undelivered_sends`] must run
    /// before this deletion to carry the rest out on the
    /// [`SessionEvent::SpawnFailed`] the caller emits. The `failed` status is
    /// kept only for the defensive case of a session that somehow already
    /// ingested messages (data worth keeping), which a never-bound spawn cannot
    /// normally reach.
    ///
    /// [`SessionEvent::SpawnFailed`]: crate::ports::SessionEvent::SpawnFailed
    pub(in crate::interactor) async fn clean_up_failed_spawn_row(
        &self,
        session_id: &delta_model::SessionId,
    ) -> Result<()> {
        if self.store.message_count(session_id).await? == 0 {
            self.store.delete_session(session_id).await?;
        } else {
            self.store.mark_session_failed(session_id).await?;
        }
        Ok(())
    }

    /// Best-effort pane teardown shared by the watchdog sweeps and the
    /// `SessionEnd` failure path: probe with `has_session` and kill if present,
    /// never letting a teardown error mask the failure report (the launch is
    /// already removed from the runtime state, so the failure event must still
    /// fire).
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

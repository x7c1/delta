//! The actor's half of the deferred launch: what happens when the background
//! launch preparation reports its outcome.

use std::time::Instant;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{LaunchTarget, PendingSpawn};
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Apply the outcome of this session's launch preparation, posted by the
    /// launch task ([`spawn_launch_preparation`]) on this actor's own mailbox.
    ///
    /// By the time this arrives the pending spawn has usually already been
    /// recorded — the task checkpoints on `LaunchPrepared` before it creates the
    /// pane (see [`Self::record_launched_pane`]) — so which record this session
    /// still holds says how far the launch got, and the three shapes are handled
    /// separately:
    ///
    /// - **Still launching**: the preparation failed (or timed out) before it
    ///   reached that checkpoint, so no agent was ever started. `Err` takes the
    ///   rollback below. `Ok` cannot happen — reaching the agent means passing
    ///   the checkpoint — so it is logged as the logic error it is and, for a
    ///   pane launch, the pending spawn is recorded anyway rather than dropping
    ///   a live pane on the floor. An adapter launch has nothing equivalent to
    ///   salvage (its checkpoint *is* the bind), so it is rolled back.
    /// - **Pending**: the pane was created and nothing has bound it yet. `Ok` is
    ///   the normal path and does nothing at all: the spawn is already recorded,
    ///   its bind deadline already stamped, and the first hook takes it from
    ///   here. `Err` (a `create_session` that failed after the checkpoint) drops
    ///   that entry and rolls back.
    /// - **Neither**: the session is already bound — the launch's first hook
    ///   claimed the pending spawn (a routine outcome now that the spawn is
    ///   recorded before the pane exists), or an adapter launch's checkpoint
    ///   bound the agent — or the session was closed/reaped meanwhile. Nothing
    ///   is left to settle. `Err` here would mean a bound session whose agent
    ///   never came up, which is not reachable, so it is warned about and
    ///   ignored.
    ///
    /// **Rollback** — a remote branch that does not exist, a `git worktree add`
    /// error, a worktree that landed on a path other than the one the accept
    /// phase planned ([`Error::WorktreeLandedElsewhere`]), a tmux failure, an
    /// adapter that would not connect or start a thread, or the whole sequence
    /// outrunning [`LaunchConfig::launch_prep_deadline`] — undoes the
    /// acceptance exactly as the synchronous launch failure used to: reclaim
    /// whatever the launch did stand up (a pane, or a connected adapter), drop
    /// the turn, delete the eager session row (its main thread and every send
    /// go by cascade). The REST caller is long gone, so the failure is reported
    /// on the async event seam instead, as a [`SessionEvent::SpawnFailed`]
    /// carrying the error text — that `reason` is the only place any of those
    /// messages can still be shown, since it is no longer a `4xx`/`5xx` body —
    /// and carrying `unsent`, the text of every send the launch accepted but
    /// never delivered, read a step before the rows cascade away.
    ///
    /// [`spawn_launch_preparation`]: super::launch_prep::spawn_launch_preparation
    /// [`LaunchConfig::launch_prep_deadline`]: crate::launch_config::LaunchConfig::launch_prep_deadline
    pub(in crate::interactor) async fn finish_launch(
        &mut self,
        token: &PaneToken,
        outcome: Result<()>,
    ) {
        if let Some(launching) = self.state.take_launching_for_token(token) {
            // A pane launch reports itself with a pane token the browser can
            // show and a tmux session to reclaim; an adapter launch has neither.
            let pane_token = match &launching.target {
                LaunchTarget::Pane(_) => Some(token),
                LaunchTarget::Adapter(_) => None,
            };
            match outcome {
                Ok(()) => match launching.target {
                    LaunchTarget::Pane(pane) => {
                        tracing::error!(
                            token = %token.as_str(),
                            session_id = %self.id,
                            workdir = %launching.workdir,
                            "a launch reported success without passing its LaunchPrepared \
                             checkpoint; recording the pending spawn so the pane can still bind"
                        );
                        self.state.push_pending(PendingSpawn {
                            token: launching.token,
                            pane: pane.pane,
                            created_at: Instant::now(),
                        });
                    }
                    LaunchTarget::Adapter(_) => {
                        // An adapter launch's checkpoint *is* its bind, so a
                        // success that never reached one left no live session
                        // behind — there is nothing to salvage, only an
                        // accepted row that would sit `spawning` forever.
                        let err = Error::Agent(format!(
                            "the adapter-backed launch of {} reported success without \
                             binding its agent",
                            self.id
                        ));
                        self.roll_back_failed_launch(None, &err).await;
                    }
                },
                Err(err) => self.roll_back_failed_launch(pane_token, &err).await,
            }
            return;
        }
        if self.state.has_pending_for_token(token) {
            match outcome {
                Ok(()) => tracing::info!(
                    token = %token.as_str(),
                    session_id = %self.id,
                    "fresh spawn launched; awaiting first UserPromptSubmit to bind"
                ),
                Err(err) => {
                    // The pane never came up, so the spawn recorded a moment
                    // ago has nothing left to bind to. Only a pane launch ever
                    // records one, so the token is always a real tmux name.
                    self.state.remove_pending_for_token(token);
                    self.roll_back_failed_launch(Some(token), &err).await;
                }
            }
            return;
        }
        match outcome {
            Ok(()) => tracing::debug!(
                token = %token.as_str(),
                session_id = %self.id,
                "a launch reported success with its spawn already settled (bound or \
                 rolled back); nothing to do"
            ),
            Err(err) => tracing::warn!(
                token = %token.as_str(),
                session_id = %self.id,
                error = %err,
                "a launch reported failure with no spawn left to roll back; ignoring it"
            ),
        }
    }

    /// Undo an accepted-but-failed launch: reclaim whatever the launch stood
    /// up, drop the turn, delete the eager session row, and announce the
    /// failure on the async event seam.
    ///
    /// Shared by every failure shape above — a preparation that died before the
    /// agent, a `create_session` that died after its checkpoint, and an adapter
    /// launch that connected but could not be bound — because the cleanup is
    /// identical once the caller has removed whichever launch record the
    /// session was holding.
    ///
    /// `pane_token` is `Some` only for a pane-backed (Claude) launch: it names
    /// the tmux session to reclaim and travels on the event so the browser can
    /// show it. An adapter-backed launch has no pane at all — passing `None`
    /// keeps tmux entirely out of its rollback (a probe against a name tmux was
    /// never given would answer "no such session" anyway, but asking at all
    /// would be a lie about what this session is).
    async fn roll_back_failed_launch(&mut self, pane_token: Option<&PaneToken>, err: &Error) {
        tracing::error!(
            token = pane_token.map(PaneToken::as_str),
            session_id = %self.id,
            error = %err,
            "fresh spawn failed to launch; rolling back the eager session row \
             and reporting SpawnFailed"
        );
        if let Some(token) = pane_token {
            // A failure before `create_session` leaves no pane at all; one after
            // it leaves a pane to reclaim. The probe-then-kill helper covers both.
            self.kill_pane_best_effort(token.as_str()).await;
        }
        // An adapter-backed launch that got as far as binding holds a live
        // provider connection (Codex: a `codex app-server` process). Close it
        // explicitly rather than relying on the drop that follows, so the
        // provider is told the thread is over and the process is reclaimed at a
        // point we can log. A no-op for a pane-backed or never-bound launch.
        if let Some(agent) = self.state.remove_open_agent() {
            if let Err(close_err) = agent.adapter.close(&agent.handle).await {
                tracing::warn!(
                    session_id = %self.id,
                    error = %close_err,
                    "failed to close the adapter of a launch that could not be \
                     completed (the connection is dropped regardless)"
                );
            }
        }
        // The session row (and every send row, by cascade) is deleted, so the
        // turn entry is dropped without orphan handling.
        self.state.forget_turn();
        let session_id = self.id.clone();
        // BEFORE the cleanup, which deletes the rows this reads.
        let unsent = self.undelivered_sends(&session_id).await;
        if let Err(cleanup_err) = self.clean_up_failed_spawn_row(&session_id).await {
            // Report the launch failure regardless: the browser is waiting on a
            // session that will never come up, and a row that outlived its
            // cleanup is the lesser problem.
            tracing::error!(
                session_id = %session_id,
                error = %cleanup_err,
                "failed to clean up the eager session row of a failed launch"
            );
        }
        self.emit_async_event(SessionEvent::SpawnFailed {
            session_id,
            pane_token: pane_token.map(|token| token.as_str().to_owned()),
            reason: Some(err.to_string()),
            unsent,
        });
    }
}

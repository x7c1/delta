//! The actor's half of the deferred launch: what happens when the background
//! launch preparation reports its outcome.

use std::time::Instant;

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::PendingSpawn;
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
    ///   reached that checkpoint, so no pane was ever created. `Err` takes the
    ///   rollback below. `Ok` cannot happen — reaching the pane means passing
    ///   the checkpoint — so it is logged as the logic error it is and the
    ///   pending spawn is recorded anyway, rather than dropping a live pane on
    ///   the floor.
    /// - **Pending**: the pane was created and nothing has bound it yet. `Ok` is
    ///   the normal path and does nothing at all: the spawn is already recorded,
    ///   its bind deadline already stamped, and the first hook takes it from
    ///   here. `Err` (a `create_session` that failed after the checkpoint) drops
    ///   that entry and rolls back.
    /// - **Neither**: the launch's first hook already bound the session — a
    ///   routine outcome now that the spawn is recorded before the pane exists —
    ///   or the session was closed/reaped meanwhile. Nothing is left to settle.
    ///   `Err` here would mean a bound session whose pane never came up, which
    ///   is not reachable, so it is warned about and ignored.
    ///
    /// **Rollback** — a remote branch that does not exist, a `git worktree add`
    /// error, a worktree that landed on a path other than the one the accept
    /// phase planned ([`Error::WorktreeLandedElsewhere`]), a tmux failure, or
    /// the whole sequence outrunning [`LaunchConfig::launch_prep_deadline`] —
    /// undoes the acceptance exactly as the synchronous launch failure used to:
    /// kill any pane that did come up (best-effort), drop the turn, delete the
    /// eager session row (its main thread and first send go by cascade). The
    /// REST caller is long gone, so the failure is reported on the async event
    /// seam instead, as a [`SessionEvent::SpawnFailed`] carrying the error text
    /// — that `reason` is the only place any of those messages can still be
    /// shown, since it is no longer a `4xx`/`5xx` body.
    ///
    /// [`spawn_launch_preparation`]: super::launch_prep::spawn_launch_preparation
    /// [`LaunchConfig::launch_prep_deadline`]: crate::launch_config::LaunchConfig::launch_prep_deadline
    pub(in crate::interactor) async fn finish_launch(
        &mut self,
        token: &PaneToken,
        outcome: Result<()>,
    ) {
        if let Some(launching) = self.state.take_launching_for_token(token) {
            match outcome {
                Ok(()) => {
                    tracing::error!(
                        token = %token.as_str(),
                        session_id = %self.id,
                        workdir = %launching.workdir,
                        "a launch reported success without passing its LaunchPrepared \
                         checkpoint; recording the pending spawn so the pane can still bind"
                    );
                    self.state.push_pending(PendingSpawn {
                        token: launching.token,
                        pane: launching.pane,
                        created_at: Instant::now(),
                    });
                }
                Err(err) => self.roll_back_failed_launch(token, &err).await,
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
                    // ago has nothing left to bind to.
                    self.state.remove_pending_for_token(token);
                    self.roll_back_failed_launch(token, &err).await;
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

    /// Undo an accepted-but-failed launch: reclaim any pane, drop the turn,
    /// delete the eager session row, and announce the failure on the async
    /// event seam.
    ///
    /// Shared by the two failure shapes above — a preparation that died before
    /// the pane and a `create_session` that died after it — because the cleanup
    /// is identical once the caller has removed whichever launch record the
    /// session was holding.
    async fn roll_back_failed_launch(&mut self, token: &PaneToken, err: &Error) {
        tracing::error!(
            token = %token.as_str(),
            session_id = %self.id,
            error = %err,
            "fresh spawn failed to launch; rolling back the eager session row \
             and reporting SpawnFailed"
        );
        // A failure before `create_session` leaves no pane at all; one after it
        // leaves a pane to reclaim. The probe-then-kill helper covers both.
        self.kill_pane_best_effort(token.as_str()).await;
        // The session row (and its first send, by cascade) is deleted, so the
        // turn entry is dropped without orphan handling.
        self.state.forget_turn();
        let session_id = self.id.clone();
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
            pane_token: token.as_str().to_owned(),
            reason: Some(err.to_string()),
        });
    }
}

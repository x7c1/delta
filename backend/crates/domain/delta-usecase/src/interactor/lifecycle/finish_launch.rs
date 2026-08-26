//! The actor's half of the deferred launch: what happens when the background
//! launch preparation reports back.

use std::time::Instant;

use crate::error::Result;
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
    /// **Success** turns the launching entry into a [`PendingSpawn`] stamped
    /// *now*: the bind watchdog's clock starts at the launch, not at
    /// acceptance, so a long `git fetch` cannot eat the deadline the first
    /// `UserPromptSubmit` has to arrive within. From here the spawn behaves
    /// exactly as it always has — the first hook binds it, or the reaper takes
    /// it.
    ///
    /// **Failure** — a remote branch that does not exist, a `git worktree add`
    /// error, a tmux failure, or the whole sequence outrunning
    /// [`LAUNCH_PREP_DEADLINE`] — rolls the acceptance back exactly as the
    /// synchronous launch failure used to: kill any pane that did come up
    /// (best-effort; usually there is none), drop the turn, delete the eager
    /// session row (its main thread and first send go by cascade). The REST
    /// caller is long gone, so the failure is reported on the async event seam
    /// instead, as a [`SessionEvent::SpawnFailed`] carrying the error text —
    /// that `reason` is the only place the git or tmux message can still be
    /// shown, since it is no longer a `4xx`/`5xx` body.
    ///
    /// A report whose token does not match the launching entry is stale (the
    /// launch was already rolled back) and is ignored.
    ///
    /// [`spawn_launch_preparation`]: super::launch_prep::spawn_launch_preparation
    /// [`LAUNCH_PREP_DEADLINE`]: super::launch_prep::LAUNCH_PREP_DEADLINE
    pub(in crate::interactor) async fn finish_launch(
        &mut self,
        token: &PaneToken,
        outcome: Result<()>,
    ) {
        let Some(launching) = self.state.take_launching_for_token(token) else {
            tracing::warn!(
                token = %token.as_str(),
                session_id = %self.id,
                "a launch reported back with no matching launching entry; ignoring it"
            );
            return;
        };
        match outcome {
            Ok(()) => {
                self.state.push_pending(PendingSpawn {
                    token: launching.token,
                    pane: launching.pane,
                    // Stamp at the launch, not at acceptance: the preparation
                    // that just finished may have taken far longer than the
                    // bind deadline allows for, and the first hook can only
                    // fire from here on.
                    created_at: Instant::now(),
                });
                tracing::info!(
                    token = %token.as_str(),
                    session_id = %self.id,
                    workdir = %launching.workdir,
                    prepared_in_ms = launching.accepted_at.elapsed().as_millis(),
                    "fresh spawn launched; awaiting first UserPromptSubmit to bind"
                );
            }
            Err(err) => {
                tracing::error!(
                    token = %token.as_str(),
                    session_id = %self.id,
                    workdir = %launching.workdir,
                    error = %err,
                    "fresh spawn failed to launch; rolling back the eager session row \
                     and reporting SpawnFailed"
                );
                // A failure before `create_session` leaves no pane at all; one
                // after it leaves a pane to reclaim. The probe-then-kill helper
                // covers both.
                self.kill_pane_best_effort(token.as_str()).await;
                // The session row (and its first send, by cascade) is deleted,
                // so the turn entry is dropped without orphan handling.
                self.state.forget_turn();
                let session_id = self.id.clone();
                if let Err(cleanup_err) = self.clean_up_failed_spawn_row(&session_id).await {
                    // Report the launch failure regardless: the browser is
                    // waiting on a session that will never come up, and a row
                    // that outlived its cleanup is the lesser problem.
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
    }
}

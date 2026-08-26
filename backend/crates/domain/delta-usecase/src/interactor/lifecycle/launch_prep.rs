//! The background half of a fresh spawn: everything between accepting a
//! new-session send and having a live `claude` pane.
//!
//! `POST /api/sends { new_session: true }` used to block for the whole launch.
//! On a large repository that is seconds to tens of seconds of a disabled
//! composer and a browser that cannot switch to the session it just created,
//! because the worktree build (`git fetch` + `git worktree add`, a full
//! checkout) sits inside the request. So the request now only *accepts* the
//! session — see [`spawn_fresh`](super::spawn_fresh) — and this module performs
//! the rest on a `tokio::spawn`ed task, reporting back through the session
//! actor's own mailbox — [`SessionInput::LaunchPrepared`] just before the pane
//! is created, then [`SessionInput::LaunchFinished`] with the outcome.
//!
//! The task never touches runtime state: it holds a
//! [`WeakUnboundedSender`](mpsc::WeakUnboundedSender) and an `Arc` of the core
//! and nothing else, exactly like the Codex event pump
//! ([`spawn_agent_event_pump`](crate::interactor::agent_event::spawn_agent_event_pump)).
//! Every mutation stays on the actor, in mailbox order, in the
//! `LaunchPrepared` handler
//! ([`record_launched_pane`](super::record_launched_pane)) and the
//! `LaunchFinished` one ([`finish_launch`](super::finish_launch)).

use std::sync::Arc;
use std::time::Duration;

use delta_model::SessionId;
use tokio::sync::{mpsc, oneshot};

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::session_actor::runtime::LaunchingSpawn;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::LaunchApproval;

/// How long the whole launch preparation may take before it is abandoned.
///
/// The sequence is unbounded from Delta's side: `git fetch origin <branch>` can
/// hang on an unreachable remote or a credential prompt with no timeout of its
/// own, and a session stuck there would sit `spawning` forever — nothing else
/// watches it, because the bind watchdog only starts once a pane exists. This
/// is that backstop. It is set far above any honest preparation (a cold clone
/// of a large repository is minutes at worst) precisely so it never truncates a
/// slow-but-healthy checkout: reaching it means the launch is stuck, and the
/// session is failed with a reason the browser can show.
pub(in crate::interactor) const LAUNCH_PREP_DEADLINE: Duration = Duration::from_secs(600);

/// Run an accepted session's launch preparation on a background task and post
/// its outcome back to the session's actor.
///
/// Fire-and-forget by design: the REST caller already has its `201`, so a
/// failure here is reported as a [`SessionEvent::SpawnFailed`] event rather
/// than returned to anyone. If the actor is gone by the time the task finishes
/// (the interactor is shutting down), the report is dropped silently — there is
/// no state left to correct.
///
/// [`SessionEvent::SpawnFailed`]: crate::ports::SessionEvent::SpawnFailed
pub(in crate::interactor) fn spawn_launch_preparation<T, X, S, W, G>(
    core: Arc<InteractorCore<T, X, S, W, G>>,
    self_sender: mpsc::WeakUnboundedSender<SessionInput>,
    session_id: SessionId,
    launching: LaunchingSpawn,
) where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    // Counted before the task starts, so a test that awaits the launch straight
    // after the accepting request returned already sees this one in flight.
    #[cfg(test)]
    core.launches_in_flight
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    tokio::spawn(async move {
        let token = launching.token.clone();
        let outcome = match tokio::time::timeout(
            LAUNCH_PREP_DEADLINE,
            core.prepare_launch(&session_id, &launching, &self_sender),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => Err(Error::LaunchPreparationTimedOut(format!(
                "the launch preparation for {} did not finish within {}s",
                launching.workdir,
                LAUNCH_PREP_DEADLINE.as_secs()
            ))),
        };
        if let Some(sender) = self_sender.upgrade() {
            let _ = sender.send(SessionInput::LaunchFinished { token, outcome });
        }
        #[cfg(test)]
        core.launches_in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Prepare and launch an accepted session, in the order the synchronous
    /// spawn used to run these steps in.
    ///
    /// 1. Build (or reuse) the requested git worktree. It must land on the path
    ///    the accept phase planned — that path is already stored as the
    ///    session's `cwd` — so a mismatch is logged loudly rather than silently
    ///    launching somewhere else.
    /// 2. Pre-accept Claude Code's workspace-trust dialog for the launch
    ///    directory when it is a git working tree. Without it `claude` opens a
    ///    blocking dialog at startup, no `UserPromptSubmit` ever fires, and the
    ///    spawn is reaped at the bind deadline.
    /// 3. Write Delta's session settings (hooks + theme) to the Delta-owned
    ///    path the launch argv points `--settings` at.
    /// 4. Check in with the session's actor ([`SessionInput::LaunchPrepared`]),
    ///    which records the [`PendingSpawn`] the launch's first hook will bind,
    ///    and await its reply. This step exists purely for its ordering: the
    ///    hooks the next step triggers land on that same mailbox, so the spawn
    ///    has to be recorded before the pane can exist — otherwise a fast agent
    ///    submits its launch prompt against a session that has nothing pending,
    ///    the hook is dismissed as external input, and the spawn recorded
    ///    afterwards has no hook left to bind it. The reply can also say
    ///    [`LaunchApproval::Abandon`] (the acceptance was rolled back while the
    ///    preparation ran), which ends the launch here with no pane created.
    /// 5. Launch the agent in its tmux pane.
    ///
    /// Any failure aborts the rest: the caller rolls the eager row back, so a
    /// half-prepared launch leaves nothing behind but the (side-effect-free)
    /// minted token and, for a failed step 5, a worktree that a retry reuses.
    ///
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    async fn prepare_launch(
        &self,
        session_id: &SessionId,
        launching: &LaunchingSpawn,
        self_sender: &mpsc::WeakUnboundedSender<SessionInput>,
    ) -> Result<()> {
        if let Some(worktree) = &launching.worktree {
            let built = self
                .resolve_worktree_launch_dir(
                    session_id,
                    &worktree.repo_root,
                    worktree.repository_display_name.as_deref(),
                    worktree.spec.clone(),
                )
                .await?;
            if built != launching.workdir {
                tracing::warn!(
                    session_id = %session_id,
                    planned = %launching.workdir,
                    built = %built,
                    "the built worktree path differs from the one planned at accept time; \
                     launching in the planned path, which the session row already records"
                );
            }
        }
        if launching.seed_trust {
            self.git_worktree
                .ensure_dir_trusted(&launching.workdir)
                .await?;
        }
        self.workspace
            .write_session_settings(&self.session_settings_path, &self.session_settings_json)
            .await?;
        if await_recorded_spawn(self_sender, launching).await == LaunchApproval::Abandon {
            return Ok(());
        }
        self.tmux
            .create_session(
                launching.token.as_str(),
                &launching.workdir,
                &launching.command,
            )
            .await
    }
}

/// Ask the session's actor to record the pending spawn, and wait for it to have
/// done so.
///
/// Returns [`LaunchApproval::Abandon`] whenever the answer cannot be had — the
/// actor is gone (the interactor is shutting down) or dropped the reply — since
/// in every such case there is no session state left for a pane to belong to,
/// which is exactly what `Abandon` means to the caller.
async fn await_recorded_spawn(
    self_sender: &mpsc::WeakUnboundedSender<SessionInput>,
    launching: &LaunchingSpawn,
) -> LaunchApproval {
    let Some(sender) = self_sender.upgrade() else {
        return LaunchApproval::Abandon;
    };
    let (reply, wait) = oneshot::channel();
    let posted = sender.send(SessionInput::LaunchPrepared {
        token: launching.token.clone(),
        reply,
    });
    if posted.is_err() {
        return LaunchApproval::Abandon;
    }
    match wait.await {
        Ok(Ok(approval)) => approval,
        // The handler is infallible, so an `Err` payload cannot occur; a closed
        // channel means the actor retired mid-launch. Both leave nothing to
        // launch into.
        Ok(Err(_)) | Err(_) => LaunchApproval::Abandon,
    }
}

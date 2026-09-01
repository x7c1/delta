//! The background half of a fresh spawn: everything between accepting a
//! new-session send and having a live agent.
//!
//! `POST /api/sends { new_session: true }` used to block for the whole launch.
//! On a large repository that is seconds to tens of seconds of a disabled
//! composer and a browser that cannot switch to the session it just created,
//! because the worktree build (`git fetch` + `git worktree add`, a full
//! checkout) sits inside the request. So the request now only *accepts* the
//! session — see [`spawn_fresh`](super::spawn_fresh) and
//! [`adapter_session`](super::adapter_session) —
//! and this module performs the rest on a `tokio::spawn`ed task, reporting back
//! through the session actor's own mailbox: a mid-launch checkpoint, then
//! [`SessionInput::LaunchFinished`] with the outcome.
//!
//! This shell is **shared by both providers**, and is the reason the split is
//! one mechanism rather than two: one deadline
//! ([`LaunchConfig::launch_prep_deadline`]), one in-flight count, one
//! `LaunchFinished` rollback. Only the tail differs
//! ([`LaunchTarget`]) — Claude seeds trust, writes its settings file and
//! creates a tmux pane (checkpointing on [`SessionInput::LaunchPrepared`] just
//! before it), while an adapter-backed provider connects and starts a thread
//! (checkpointing on [`SessionInput::AdapterLaunchPrepared`] with the live
//! connection). The worktree build in front of both is shared outright.
//!
//! The task never touches runtime state: it holds a
//! [`WeakUnboundedSender`](mpsc::WeakUnboundedSender) and an `Arc` of the core
//! and nothing else, exactly like the Codex event pump
//! ([`spawn_agent_event_pump`](crate::interactor::agent_event::spawn_agent_event_pump)).
//! Every mutation stays on the actor, in mailbox order, in the checkpoint
//! handlers ([`record_launched_pane`](super::record_launched_pane),
//! [`adapter_launch`](super::adapter_launch)) and the `LaunchFinished` one
//! ([`finish_launch`](super::finish_launch)).
//!
//! [`LaunchConfig::launch_prep_deadline`]: crate::launch_config::LaunchConfig::launch_prep_deadline
//! [`SessionInput::AdapterLaunchPrepared`]: crate::interactor::session_actor::input::SessionInput::AdapterLaunchPrepared

use std::sync::Arc;

use delta_model::SessionId;
use tokio::sync::{mpsc, oneshot};

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::session_actor::runtime::{LaunchTarget, LaunchingSpawn, PaneLaunch};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::LaunchApproval;

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
        let deadline = core.launch.launch_prep_deadline;
        let outcome = match tokio::time::timeout(
            deadline,
            core.prepare_launch(&session_id, &launching, &self_sender),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => Err(Error::LaunchPreparationTimedOut(format!(
                "the launch preparation for {} did not finish within {deadline:?}",
                launching.workdir,
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
    /// Prepare and launch an accepted session: the shared worktree build, then
    /// the chosen provider's tail.
    ///
    /// The build comes first for both providers and must land on the path the
    /// accept phase planned — that path is already stored as the session's
    /// `cwd` — so a mismatch fails the launch
    /// ([`Error::WorktreeLandedElsewhere`]) rather than starting the agent in a
    /// directory that was never created. See that variant for the one start
    /// point that can diverge and why no repair is available.
    ///
    /// Any failure aborts the rest: the caller rolls the eager row back, so a
    /// half-prepared launch leaves nothing behind but the (side-effect-free)
    /// launch key and, for a failure after the build, a worktree that a retry
    /// reuses.
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
                return Err(Error::WorktreeLandedElsewhere {
                    branch: worktree.branch.clone(),
                    planned: launching.workdir.clone(),
                    built,
                });
            }
        }
        match &launching.target {
            LaunchTarget::Pane(pane) => {
                self.prepare_pane_launch(launching, pane, self_sender).await
            }
            LaunchTarget::Adapter(spec) => {
                self.prepare_adapter_launch(session_id, launching, spec, self_sender)
                    .await
            }
        }
    }

    /// The Claude tail of a launch, once the worktree is in place.
    ///
    /// 1. Pre-accept Claude Code's workspace-trust dialog for the launch
    ///    directory when it is a git working tree. Without it `claude` opens a
    ///    blocking dialog at startup, no `UserPromptSubmit` ever fires, and the
    ///    spawn is reaped at the bind deadline.
    /// 2. Write Delta's session settings (hooks + theme) to the Delta-owned
    ///    path the launch argv points `--settings` at.
    /// 3. Check in with the session's actor ([`SessionInput::LaunchPrepared`]),
    ///    which records the [`PendingSpawn`] the launch's first hook will bind,
    ///    and await its reply. This step exists purely for its ordering: the
    ///    hooks the next step triggers land on that same mailbox, so the spawn
    ///    has to be recorded before the pane can exist — otherwise a fast agent
    ///    submits its launch prompt against a session that has nothing pending,
    ///    the hook is dismissed as external input, and the spawn recorded
    ///    afterwards has no hook left to bind it. The reply can also say
    ///    [`LaunchApproval::Abandon`] (the acceptance was rolled back while the
    ///    preparation ran), which ends the launch here with no pane created.
    /// 4. Launch the agent in its tmux pane.
    ///
    /// [`PendingSpawn`]: crate::interactor::session_actor::runtime::PendingSpawn
    async fn prepare_pane_launch(
        &self,
        launching: &LaunchingSpawn,
        pane: &PaneLaunch,
        self_sender: &mpsc::WeakUnboundedSender<SessionInput>,
    ) -> Result<()> {
        // Pre-accept the trust dialog only for a git working tree (`seed_trust`)
        // that also lives under Delta's own worktree base — i.e. a worktree
        // Delta created. A user-selected repo is trust-eligible too, but seeding
        // it would silently trust its checked-in automation in the user's plain
        // `claude` sessions, so it gets Claude Code's normal dialog instead. See
        // [`super::is_under_worktree_base`] for the trade-off.
        if pane.seed_trust && super::is_under_worktree_base(&self.worktree_base, &launching.workdir)
        {
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
            .create_session(launching.token.as_str(), &launching.workdir, &pane.command)
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

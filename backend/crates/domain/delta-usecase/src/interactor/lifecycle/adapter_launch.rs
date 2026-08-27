//! The adapter-backed half of the deferred launch: connecting the provider off
//! the actor, then checking in to have the session bound on it.
//!
//! The counterpart of [`record_launched_pane`](super::record_launched_pane) for
//! a terminal-less provider (Codex), and the mirror image of it: a pane launch
//! checks in *before* the pane exists (so the hook that binds it cannot arrive
//! first) and binds afterwards via that hook, while an adapter launch connects
//! first and checks in *with the live connection*, because the bind has nothing
//! to wait for — the handle in hand is the session.
//!
//! The split follows the same rule as everywhere else in the actor design:
//! anything slow or state-free runs on the launch task ([`connect_adapter_agent`]
//! — `codex app-server` plus its handshake plus `thread/start`, seconds on a
//! cold start), and anything that mutates [`SessionRuntime`] or needs the
//! actor's own mailbox handle runs on the actor, in mailbox order
//! ([`SessionContext::activate_adapter_session`]). Blocking the mailbox on
//! `connect` would stall every other signal for that session, including the
//! `409` a send arriving in the launch window must get.
//!
//! [`connect_adapter_agent`]: crate::interactor::InteractorCore::connect_adapter_agent
//! [`SessionRuntime`]: crate::interactor::session_actor::runtime::SessionRuntime

use std::sync::Arc;

use delta_model::SessionId;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{AgentAdapter, AgentSessionHandle};
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::session_actor::runtime::{AdapterLaunch, LaunchTarget, LaunchingSpawn};
use crate::interactor::InteractorCore;
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::adapter_session::AdapterBind;
use super::LaunchApproval;

/// A connected, thread-started adapter handed from the launch task to the
/// actor, for the actor to bind as the session's live agent.
///
/// It travels by message rather than being installed by the task because
/// binding mutates the session's runtime state and spawns the event pump on the
/// actor's own mailbox — see the module docs.
pub(in crate::interactor) struct PreparedAdapterLaunch {
    /// The live adapter, already connected (Codex: the `codex app-server`
    /// process and its handshake). If the actor never binds it, dropping this
    /// tears the connection — and its process — down.
    pub adapter: Arc<dyn AgentAdapter>,
    /// The provider's handle for the thread `launch` just started.
    pub handle: AgentSessionHandle,
    /// The branch observed in the (now built) launch directory, for stamping on
    /// the messages this session produces. Observed on the task, where the
    /// worktree build that created the directory just finished.
    pub git_branch: Option<String>,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Bind an adapter-backed launch that has connected and started its provider
    /// thread — the handler for [`SessionInput::AdapterLaunchPrepared`].
    ///
    /// It takes the [`LaunchingSpawn`] the accept phase recorded and turns it
    /// into a live session ([`Self::activate_adapter_session`]). Unlike the pane
    /// path there is no intermediate `PendingSpawn`: nothing has to arrive
    /// afterwards to bind this session, so the entry is consumed rather than
    /// swapped, and the later `LaunchFinished(Ok)` finds the launch already
    /// settled.
    ///
    /// Keyed by `token` so a report from a launch that was already rolled back
    /// cannot bind an unrelated one; that case answers
    /// [`LaunchApproval::Abandon`], and the task then drops the adapter it was
    /// holding, closing the provider connection.
    ///
    /// On failure the launching entry is **put back** before the error is
    /// returned, so the `LaunchFinished(Err)` that follows finds it and runs the
    /// one shared rollback (row deleted, adapter closed, `SpawnFailed` emitted)
    /// rather than warning about a report with nothing left to settle.
    pub(in crate::interactor) async fn bind_adapter_launch(
        &mut self,
        token: &PaneToken,
        prepared: PreparedAdapterLaunch,
    ) -> Result<LaunchApproval> {
        let Some(launching) = self.state.take_launching_for_token(token) else {
            tracing::warn!(
                token = %token.as_str(),
                session_id = %self.id,
                "a prepared adapter launch has no matching launching entry; \
                 abandoning it rather than binding a session that was rolled back"
            );
            return Ok(LaunchApproval::Abandon);
        };
        // Only an adapter launch posts this checkpoint, so a mismatch is a
        // routing bug rather than a race: put the entry back untouched and
        // abandon, so `LaunchFinished` still finds it to settle.
        if !matches!(launching.target, LaunchTarget::Adapter(_)) {
            tracing::error!(
                token = %token.as_str(),
                session_id = %self.id,
                "a pane launch reported an adapter checkpoint; abandoning it"
            );
            self.state.start_launching(launching);
            return Ok(LaunchApproval::Abandon);
        }
        let LaunchTarget::Adapter(spec) = &launching.target else {
            unreachable!("the guard above rejected every non-adapter target")
        };
        let PreparedAdapterLaunch {
            adapter,
            handle,
            git_branch,
        } = prepared;
        match self
            .activate_adapter_session(&launching, spec, adapter, handle, git_branch)
            .await
        {
            Ok(()) => Ok(LaunchApproval::Proceed),
            Err(err) => {
                // Restore the entry so the `LaunchFinished(Err)` this error
                // produces takes the standard rollback path — which also closes
                // the adapter, whether or not the bind got as far as installing
                // it.
                self.state.start_launching(launching);
                Err(err)
            }
        }
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
    /// Stand the provider up for an accepted adapter-backed session, then have
    /// the actor bind it.
    ///
    /// Runs on the launch task, after the shared worktree build:
    ///
    /// 1. Resolve the provider's factory. The accept phase already checked it is
    ///    registered, so an absent one here means the registry changed under the
    ///    launch — reported as a launch failure rather than silently skipped.
    /// 2. `connect` + `launch` (Codex: spawn `codex app-server`, handshake,
    ///    `thread/start`). This is the expensive part, and the reason the whole
    ///    step is off the mailbox.
    /// 3. Observe the launch directory's branch. It exists by now — the shared
    ///    build made it — which is why this is not done at accept time.
    /// 4. Check in with the actor, which binds the connection as the session's
    ///    live agent and dispatches the accepted first prompt.
    ///
    /// A failure at any step propagates to `LaunchFinished(Err)`, which deletes
    /// the eager row and emits `SpawnFailed`. Whatever this function is still
    /// holding is dropped on the way out — for a connected adapter that closes
    /// the provider connection and reclaims its process (`kill_on_drop`), so a
    /// `connect` that succeeded before a later step failed leaves nothing
    /// running.
    pub(in crate::interactor) async fn prepare_adapter_launch(
        &self,
        session_id: &SessionId,
        launching: &LaunchingSpawn,
        spec: &AdapterLaunch,
        self_sender: &mpsc::WeakUnboundedSender<SessionInput>,
    ) -> Result<()> {
        let factory = self.adapter_backed_factory(spec.provider).ok_or_else(|| {
            Error::Agent(format!(
                "no {:?} adapter factory is wired into the interactor",
                spec.provider
            ))
        })?;
        let (adapter, handle) = self
            .connect_adapter_agent(
                &factory,
                session_id,
                AdapterBind::Launch {
                    launch_options: spec.launch_options.clone(),
                },
                &launching.workdir,
                // Non-`None` exactly when this spawn planned a worktree, so
                // nothing is claimed for a plain launch directory.
                launching
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.repo_root.clone()),
            )
            .await?;
        let git_branch = self
            .observe_launch_branch(session_id, &launching.workdir)
            .await;
        let prepared = PreparedAdapterLaunch {
            adapter,
            handle,
            git_branch,
        };
        await_bound_adapter(self_sender, &launching.token, prepared).await?;
        Ok(())
    }
}

/// Ask the session's actor to bind the connected adapter, and wait for it to
/// have done so.
///
/// Unlike its pane counterpart (`await_recorded_spawn`) this can fail: the bind
/// persists the provider ids and dispatches the first prompt, so the actor's
/// answer is a `Result`. The error is returned as the launch's own, which puts
/// it on the `SpawnFailed` the browser shows.
///
/// [`LaunchApproval::Abandon`] — and every case where the answer cannot be had
/// (the actor is gone, or dropped the reply) — ends the launch quietly: there
/// is no session state left for the connection to belong to, so it is dropped
/// (closing the provider) and the launch reports success with nothing to settle.
async fn await_bound_adapter(
    self_sender: &mpsc::WeakUnboundedSender<SessionInput>,
    token: &PaneToken,
    prepared: PreparedAdapterLaunch,
) -> Result<LaunchApproval> {
    let Some(sender) = self_sender.upgrade() else {
        return Ok(LaunchApproval::Abandon);
    };
    let (reply, wait) = oneshot::channel();
    let posted = sender.send(SessionInput::AdapterLaunchPrepared {
        token: token.clone(),
        prepared: Box::new(prepared),
        reply,
    });
    if posted.is_err() {
        return Ok(LaunchApproval::Abandon);
    }
    match wait.await {
        Ok(result) => result,
        // A closed channel means the actor retired mid-launch, leaving nothing
        // to bind into.
        Err(_) => Ok(LaunchApproval::Abandon),
    }
}

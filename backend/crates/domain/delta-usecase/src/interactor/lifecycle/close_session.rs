use delta_model::SessionStatus;

use crate::error::{Error, Result};
use crate::interactor::lifecycle::{PaneTeardown, UnboundLaunchEnd};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::LaunchTarget;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// The `reason` a launch cancelled by an explicit close reports — carried on
/// the event and persisted on the row, which is where the failed session's own
/// screen reads it.
///
/// A launch the user cancelled is not a breakage, but it ends in exactly the
/// state a failed one does (nothing bound, the row marked `failed` with its
/// undelivered sends still on it), so it reuses that report — see
/// [`SessionContext::close_session`]. The event's `cancelled` flag is what
/// tells the two apart; this text just says plainly what happened, so a reader
/// who meets only the persisted reason still sees a close rather than a
/// breakage.
const CLOSED_WHILE_STARTING: &str = "closed while starting";

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Close a session. This has two outcomes, decided by whether the session
    /// ever bound an agent.
    ///
    /// **A bound session is torn down and kept**: its final transcript line is
    /// captured, its pane killed (or its adapter closed) and its binding
    /// dropped. The conversational data remains in the store and the session can
    /// be reopened. Closing a known session that is *already closed* is a no-op
    /// (it has no live pane to tear down), but an *unknown* id is a clean
    /// `SessionNotFound` (404) — the same rejection [`Self::open_session`] gives
    /// — so the browser can tell "already closed" apart from "no such session"
    /// rather than having a stale id silently succeed.
    ///
    /// **A session that is still *starting* has its launch cancelled and its row
    /// removed**, because it holds nothing to keep: the row was written eagerly
    /// when the send was accepted and no transcript line has been ingested
    /// against it, so there is no conversation to tear down *from*. Cancelling
    /// is the same outcome the launch watchdogs already produce, and it is
    /// reported the same way — a [`SessionEvent::SpawnFailed`] marked
    /// `cancelled`, whose `reason` names the close, carrying every send the
    /// launch never delivered so the browser can put that text back in front of
    /// the user (see [`Self::cancel_unbound_launch`]). Without this, a launch
    /// that wedges past every deadline would leave a card the user has no way
    /// to be rid of.
    ///
    /// The starting window has three runtime shapes, each cancelled where it
    /// stands:
    ///
    /// 1. **Launching** — the preparation is still running, so there is nothing
    ///    to reclaim yet and taking the launching entry *is* the cancellation
    ///    (see [`SessionRuntime::take_launching`] for why the still-running
    ///    preparation then stands nothing up). A worktree the build was part-way
    ///    through creating may still land — accepted here exactly as the
    ///    preparation deadline accepts it.
    /// 2. **Pending** — the pane is up and no hook has bound it. The pane is
    ///    killed and the acceptance rolled back: the spawn half of
    ///    [`Self::reap_stale_launch`], with a reason instead of silence.
    /// 3. **The row still says `spawning` with no launch record left** — a bind
    ///    whose row activation failed (unreachable now that a spawn stays
    ///    pending until its registration succeeds, but rows left that way by an
    ///    older build can still exist), and — the live case — a `spawning` row
    ///    stranded by a server restart mid-launch: the runtime is rebuilt empty,
    ///    so nothing is pending for the watchdog to reap and the row has no
    ///    launch behind it at all. Keyed on the row's status rather than on
    ///    having torn a pane down, so both are covered: the tear-down above
    ///    runs (a no-op when there is nothing bound), then the still-`spawning`
    ///    row is cleaned up and reported like the other two, so such a card
    ///    cannot stay amber with nothing behind it. This is the one shape whose
    ///    session may already hold ingested messages, in which case
    ///    [`Self::clean_up_failed_spawn_row`] keeps the row as `failed` instead
    ///    of deleting it — the report is the same, the data is not thrown away.
    ///
    /// The teardown of a bound session is [`Self::tear_down_bound_session`],
    /// shared with the background tick that closes a session whose pane has
    /// gone ([`Self::close_if_pane_vanished`]) — the two must leave a session in
    /// exactly the same state, and killing the pane is the only step this path
    /// adds. See that routine for what each step is for, including the last
    /// transcript sync that catches a line Claude Code flushed after `Stop`.
    ///
    /// Returns the events for the caller to broadcast, in the order the teardown
    /// produced them: a [`SessionEvent::PermissionResolved`] for every dialog
    /// the close stranded (nobody can answer a request whose agent is gone),
    /// then any [`SessionEvent::SubagentFinished`]s produced by the process-gone
    /// sweep (see [`Self::sweep_running_subagents_on_process_gone`]) — closing
    /// tears down the `claude` process, so a lingering background subagent's
    /// completion notification can no longer arrive to clear its indicator, and
    /// the sweep clears it here instead — plus the `SpawnFailed` of a cancelled
    /// launch. The caller announces `SessionClosed` after all of them.
    ///
    /// [`SessionRuntime::take_launching`]: crate::interactor::session_actor::runtime::SessionRuntime::take_launching
    pub(in crate::interactor) async fn close_session(&mut self) -> Result<Vec<SessionEvent>> {
        let Some(session) = self.store.session(self.id).await? else {
            return Err(Error::SessionNotFound(self.id.as_str().to_owned()));
        };
        // A launch that has not bound yet is cancelled rather than torn down.
        // Both shapes return early: nothing has been ingested, so there is no
        // transcript to sync, no binding to drop and no subagent to sweep.
        if let Some(launching) = self.state.take_launching() {
            // A pane launch names a tmux session that may not exist yet; an
            // adapter launch has no pane at all. The shared rollback covers
            // both — it probes before it kills.
            let pane_token = match &launching.target {
                LaunchTarget::Pane(_) => Some(&launching.token),
                LaunchTarget::Adapter(_) => None,
            };
            tracing::info!(
                token = %launching.token.as_str(),
                session_id = %self.id,
                workdir = %launching.workdir,
                "closing a session whose launch preparation is still running; \
                 cancelling the launch and removing the eager row"
            );
            let event = self
                .cancel_unbound_launch(
                    pane_token,
                    UnboundLaunchEnd::Cancelled(CLOSED_WHILE_STARTING.to_owned()),
                )
                .await;
            return Ok(vec![event]);
        }
        if let Some(spawn) = self.state.take_unbound_pending() {
            tracing::info!(
                token = %spawn.token.as_str(),
                session_id = %self.id,
                "closing a session whose pane is up but still unbound; killing \
                 the pane and removing the eager row"
            );
            let event = self
                .cancel_unbound_launch(
                    Some(&spawn.token),
                    UnboundLaunchEnd::Cancelled(CLOSED_WHILE_STARTING.to_owned()),
                )
                .await;
            return Ok(vec![event]);
        }
        // The bound teardown, shared with the pane-gone close that the
        // background tick performs: one last transcript sync, the binding
        // dropped (the pane killed, since closing is what ends the agent that
        // is still running in it), the permission requests the close strands
        // settled, the turn closed and the lingering background subagents
        // swept. The released handle is kept — not just dropped — so the
        // defensive cleanup below can still name the pane it tore down on the
        // event it reports.
        let (closed_pane, mut events) = self
            .tear_down_bound_session(&session, PaneTeardown::Kill)
            .await?;
        if session.status == SessionStatus::Spawning {
            // Shape 3: the row never left `spawning` and no launch record is
            // left to take — a bind that failed to activate the row, or a row
            // stranded by a restart. Whatever pane there was is already gone
            // (torn down just above, or never in this runtime at all), so the
            // rollback has only the row and the undelivered sends left to deal
            // with — and the same `SpawnFailed` gets the stuck card off the
            // list. A token, when this runtime had one, still travels on the
            // event for the browser's sake; the rollback's probe finds the pane
            // killed and leaves tmux alone.
            tracing::warn!(
                session_id = %self.id,
                had_pane = closed_pane.is_some(),
                "closing a session whose row never left `spawning`; \
                 cleaning up the row and reporting SpawnFailed"
            );
            let pane_token = closed_pane.as_ref().map(|handle| &handle.token);
            events.push(
                self.cancel_unbound_launch(
                    pane_token,
                    UnboundLaunchEnd::Cancelled(CLOSED_WHILE_STARTING.to_owned()),
                )
                .await,
            );
        }
        Ok(events)
    }
}

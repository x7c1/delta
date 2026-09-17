//! The teardown a **bound** session goes through when its agent stops driving
//! it, shared by every path that reaches that point.
//!
//! Two paths do: the explicit [`close_session`], and the background probe that
//! finds an open session's pane gone ([`close_if_pane_vanished`]). They differ
//! in exactly one step — whether the pane is still there to kill — so that is
//! the only thing [`PaneTeardown`] parameterises. Everything else (the last
//! sync, dropping the binding, settling the pending permission requests,
//! closing the turn, sweeping the background subagents) is identical, and
//! identical for a reason: a session closed by either route must be left in the
//! same state, or a send that resumes it afterwards would behave differently
//! depending on how it closed.
//!
//! [`close_session`]: SessionContext::close_session
//! [`close_if_pane_vanished`]: SessionContext::close_if_pane_vanished

use delta_model::Session;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::OpenHandle;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// The `decision_reason` recorded on a permission request that was still
/// awaiting an answer when its session was torn down — the close-side
/// counterpart of the adapter path's `PERMISSION_DENIED_ON_SESSION_DEATH`
/// (`settle_pending_permissions` says what the reason is for).
///
/// Both callers record this same sentence deliberately: a person pressing Close
/// and the sweep finding the pane gone leave the session in exactly the same
/// state, and from the request's seat the cause is identical.
const PERMISSION_DENIED_ON_SESSION_CLOSE: &str =
    "the session was closed before this request could be answered";

/// What the teardown should do about the session's tmux pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::interactor) enum PaneTeardown {
    /// Kill the pane as part of the teardown: the agent is still running in it
    /// and closing the session is what ends it (the explicit close).
    Kill,
    /// Leave tmux alone: the pane is already gone, which is *why* the session
    /// is being closed. A `kill-session` here would only fail on a name that no
    /// longer exists.
    AlreadyGone,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Tear a bound session down, keeping its data, and return the pane handle
    /// it released (if any) alongside the events the caller must broadcast.
    ///
    /// In order:
    ///
    /// 1. **One last transcript sync.** Once closed, the session loses its live
    ///    pane and the background tail no longer polls it — but Claude Code may
    ///    flush the turn's final assistant line to the JSONL just *after* its
    ///    `Stop` hook fired, so without this the straggler would never be
    ///    ingested. It runs while the on-disk transcript still reflects this
    ///    session's own run, and it is safe on a session that is already closed
    ///    (it simply finds no new lines).
    /// 2. **Drop the binding**, killing the pane when `pane` says to.
    /// 3. **Close a terminal-less agent session** through its adapter, which
    ///    tears down that session's local plumbing (the shared provider
    ///    connection stays up for other threads). A pane-backed session holds no
    ///    `open_agent`, so this is a no-op for it — and a session reached
    ///    through [`PaneTeardown::AlreadyGone`] is pane-backed by definition.
    /// 4. **Settle the pending permission requests.** Every dialog still
    ///    awaiting an answer is denied with
    ///    [`PERMISSION_DENIED_ON_SESSION_CLOSE`] and announced as resolved
    ///    through the shared settle ([`Self::settle_pending_permissions`]) — the
    ///    same routine an adapter-backed session's death runs. The agent that
    ///    raised them is gone, so nobody can answer them: leaving them would
    ///    leave the row `pending` forever, the hook parked until its own
    ///    deadline, and a dialog the browser re-raises over a closed session
    ///    where Allow can only answer `409`. That this runs *after* step 1 is
    ///    load-bearing: a `tool_result` the sync just ingested has already
    ///    settled its own row with the disposition it really got, so the deny
    ///    reaches only rows nothing can ever answer.
    /// 5. **Feed [`TurnInput::Close`]** into the turn machine: the agent can no
    ///    longer progress whatever turn was in flight, so an unechoed
    ///    outstanding send is cancelled and an in-flight one that never matched
    ///    is swept.
    /// 6. **Sweep the surviving background subagents.** The agent process is
    ///    gone, so no more of this session's transcript is ingested and a
    ///    lingering background subagent's completion `<task-notification>` can
    ///    never be folded to clear its indicator. `Close` above swept the
    ///    foreground entries; this clears the background ones, returning a
    ///    [`SessionEvent::SubagentFinished`] per entry.
    ///
    /// The returned events are in that order — the permission resolutions, then
    /// the subagent sweep's — and every caller announces the session's own close
    /// *after* them, so nothing about the session follows the news that it is
    /// closed.
    ///
    /// The returned handle is what the caller names the pane by afterwards
    /// (`close_session` puts it on the `SpawnFailed` of a row left `spawning`),
    /// which is why the binding is returned rather than merely dropped.
    ///
    /// [`TurnInput::Close`]: crate::turn::TurnInput::Close
    pub(in crate::interactor) async fn tear_down_bound_session(
        &mut self,
        session: &Session,
        pane: PaneTeardown,
    ) -> Result<(Option<OpenHandle>, Vec<SessionEvent>)> {
        self.sync_transcript(session).await?;
        let closed_pane = self.state.remove_open();
        if let Some(handle) = &closed_pane {
            if pane == PaneTeardown::Kill {
                self.tmux.kill_session(handle.token.as_str()).await?;
            }
        }
        if let Some(agent) = self.state.remove_open_agent() {
            agent.adapter.close(&agent.handle).await?;
        }
        // Deliberate no-op for git worktrees (MVP): a session that started in a
        // worktree keeps it on close. `session.cwd` is the worktree path, so a
        // later resume reattaches to the still-present worktree rather than
        // recreating it. Removing the worktree (and its branch) here would throw
        // away uncommitted work the moment a session is closed.

        // Settled BEFORE the turn closes, because of the mirror: a dialog cannot
        // outlive its turn, so `TurnInput::Close` returns the turn to idle and
        // drops the whole pending queue silently (`SessionRuntime::apply_turn`)
        // — a settle after it would find nothing left to announce and the
        // browser would keep the dialog it was shown. The settle's other three
        // pieces of state are order-free here: `Close` touches neither the
        // parked hook waiters nor the routing index nor the rows.
        //
        // The pending QUESTION needs no settle of its own. `AskUserQuestion` is
        // recorded on the same `permission_request` table as a dialog (see
        // `on_pre_tool_use`), so the store sweep inside the settle denies its
        // row too, and the `PermissionResolved` it produces for that id clears
        // both the runtime mirror (`SessionRuntime::resolve_pending_question`)
        // and the browser's question card — the same signal an answered or
        // cancelled question settles through.
        let mut events = self
            .settle_pending_permissions(PERMISSION_DENIED_ON_SESSION_CLOSE)
            .await;

        self.apply_turn_input(crate::turn::TurnInput::Close).await?;
        events.extend(self.sweep_running_subagents_on_process_gone().await?);
        Ok((closed_pane, events))
    }
}

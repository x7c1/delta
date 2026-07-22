//! Interrupting a session's in-flight turn from the API surface.
//!
//! Unlike a close, an interrupt aborts the *current turn* while leaving the
//! session open: for a terminal-less agent (Codex) it drives the adapter's
//! `interrupt` (which sends `turn/interrupt` on the provider's wire) but keeps
//! the open agent — and therefore its event pump — alive, so the provider's
//! `turn/completed{interrupted}` can arrive and settle the turn. That settle is
//! what emits [`SessionEvent::TurnInterrupted`] on the async seam; there is no
//! synchronous event to return here.
//!
//! Runs inside the session's actor, so an interrupt is ordered against the
//! session's other work (its event pump, permission decisions, and any queued
//! sends) on the one mailbox.

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Interrupt the session's in-flight turn.
    ///
    /// Only an adapter-backed (Codex) session is driven here: it borrows the
    /// open agent **non-destructively** (via [`SessionRuntime::open_agent`], not
    /// [`SessionRuntime::remove_open_agent`]) and calls
    /// [`AgentAdapter::interrupt`]. Keeping the open agent is the whole point —
    /// the session, its content source, and its event pump must survive so the
    /// provider's `turn/completed{interrupted}` can be ingested and drive the
    /// turn machine to [`SessionEvent::TurnInterrupted`] on the async seam. This
    /// is the deliberate difference from `close_session`, which removes the open
    /// agent and tears the pump down.
    ///
    /// For a pane-backed (Claude) or closed session there is no open agent, so
    /// this is a well-defined no-op: Claude's turn interrupt is TUI-driven
    /// (Escape in the pane) with its own transcript interrupt-marker path (see
    /// the sync module), which this REST path deliberately does not duplicate.
    ///
    /// [`SessionRuntime::open_agent`]: crate::interactor::session_actor::runtime::SessionRuntime::open_agent
    /// [`SessionRuntime::remove_open_agent`]: crate::interactor::session_actor::runtime::SessionRuntime::remove_open_agent
    /// [`AgentAdapter::interrupt`]: crate::agent::AgentAdapter::interrupt
    /// [`SessionEvent::TurnInterrupted`]: crate::ports::SessionEvent::TurnInterrupted
    pub(in crate::interactor) async fn interrupt(&mut self) -> Result<()> {
        let Some(agent) = self.state.open_agent().cloned() else {
            tracing::debug!(
                session_id = %self.id,
                "interrupt: no open agent (Claude/pane-backed or closed session); \
                 no-op (Claude interrupt is TUI-driven via the pane)"
            );
            return Ok(());
        };
        agent.adapter.interrupt(&agent.handle).await
    }
}

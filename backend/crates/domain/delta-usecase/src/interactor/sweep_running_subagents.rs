//! Sweeping lingering running subagents when the session's `claude` process is
//! confirmed gone.
//!
//! The "Subagent running" indicator is lit from parent-transcript ingest: for
//! every `Agent`/`Task` tool_use, and for the `<forked-skill-launch>` element a
//! harness-forked skill leaves instead of one (always a BACKGROUND entry, so
//! this sweep is the only thing that can clear it once the process is gone). A
//! FOREGROUND entry is swept when the turn returns to idle; a BACKGROUND entry
//! deliberately outlives its launching turn and is cleared only when its
//! completion `<task-notification>` is folded (`Effect::SubagentCompleted` →
//! [`SessionRuntime::finish_subagent`]). That notification-driven clear works
//! only while the process is alive to keep producing transcript lines.
//!
//! Once the process is gone no more of this session's transcript is ingested,
//! so a background entry's notification can never be folded — its indicator
//! would stay lit forever. This helper is invoked from the two graceful
//! "process gone" signals (`on_session_end`'s normal-end path and
//! `close_session`) to clear whatever running entries remain, emitting the
//! events and dropping the persisted state a live-only clear would have.
//!
//! [`SessionRuntime::finish_subagent`]:
//!     crate::interactor::session_actor::runtime::SessionRuntime::finish_subagent

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Sweep any running-subagent entries left over now that the session's
    /// `claude` process is confirmed gone, returning a
    /// [`SessionEvent::SubagentFinished`] for each so the caller can broadcast
    /// them to live viewers.
    ///
    /// Call this AFTER the `TurnInput::Close` transition: `Close` already swept
    /// the foreground entries (a foreground subagent cannot outlive its turn),
    /// so what remains here is the BACKGROUND entries whose completion
    /// `<task-notification>` can no longer arrive. For each drained entry this:
    ///
    /// - emits `SubagentFinished` (mirroring the `Effect::SubagentCompleted`
    ///   sync path) so a live viewer's indicator / unread badge clears
    ///   immediately rather than waiting for a notification that never comes;
    /// - calls [`SessionStore::clear_subagent_launch`] to drop the persisted
    ///   launch row, so a later stray notification cannot double-fire and a
    ///   resume cannot resurrect the entry from persisted state.
    ///
    /// On the resume point: a resume never rebuilds the in-memory
    /// `running_subagents` set from persisted launch rows. `sync_transcript`
    /// reads only NEW lines past a per-session line cursor, and the line that
    /// lit the indicator — the `Agent`/`Task` tool_use, or the
    /// `<forked-skill-launch>` element — sits behind that cursor, so it is never
    /// re-folded; the persisted launch rows are read back only by
    /// `outstanding_subagent_launches`, which reseeds the attribution fold's
    /// thread map for a late completion notification, not the indicator set.
    /// Clearing the launch row here therefore removes the last trace, and there
    /// is no path that would re-light a swept entry on resume.
    ///
    /// This is the shared body behind both process-gone call sites; keeping it
    /// in one place ensures they emit and persist identically.
    pub(in crate::interactor) async fn sweep_running_subagents_on_process_gone(
        &mut self,
    ) -> Result<Vec<SessionEvent>> {
        let drained = self.state.drain_running_subagents();
        let mut events = Vec::with_capacity(drained.len());
        for subagent in drained {
            tracing::info!(
                session_id = %self.id,
                tool_use_id = %subagent.tool_use_id,
                background = subagent.background,
                "clearing a lingering running subagent because the session's process \
                 is gone; its completion notification can no longer arrive"
            );
            self.store
                .clear_subagent_launch(self.id, &subagent.tool_use_id)
                .await?;
            events.push(SessionEvent::SubagentFinished {
                session_id: self.id.clone(),
                tool_use_id: subagent.tool_use_id,
            });
        }
        Ok(events)
    }
}

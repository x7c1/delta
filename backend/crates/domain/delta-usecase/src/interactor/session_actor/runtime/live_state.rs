//! [`SessionLiveState`]: one consistent snapshot of the runtime state the
//! sends envelope reports.

use delta_model::ThreadId;

use crate::turn::TurnState;

use super::{PendingPermission, PendingQuestion, RunningSubagent, SessionRuntime};

/// One consistent snapshot of the runtime state the sends envelope reports:
/// the turn phase plus the pending permission dialog, the pending question, and
/// the set of running subagents, read in a single actor message so they can
/// never disagree within one response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLiveState {
    pub turn: TurnState,
    /// The thread the in-flight turn is running on, when a turn is in flight.
    /// `None` while idle. Lets a reconnecting client re-seed its per-thread
    /// running indicator on the exact thread (main or a branch) without waiting
    /// for the next turn-lifecycle event.
    pub in_progress_thread: Option<ThreadId>,
    pub pending_permission: Option<PendingPermission>,
    pub pending_question: Option<PendingQuestion>,
    /// The subagents currently running in this session's turn, oldest first.
    pub running_subagents: Vec<RunningSubagent>,
}

impl SessionRuntime {
    /// Snapshot the queryable live state (turn phase + pending permission +
    /// pending question + running subagents) in one read, for the sends
    /// envelope.
    /// The `in_progress_thread` is left `None` here and filled in by the actor
    /// handler, which has the store needed to resolve the in-flight turn's
    /// thread; this snapshot owns only the runtime fields it already holds.
    pub fn live_state(&self) -> SessionLiveState {
        SessionLiveState {
            turn: self.turn,
            in_progress_thread: None,
            pending_permission: self.pending_permission.clone(),
            pending_question: self.pending_question.clone(),
            running_subagents: self.running_subagents.clone(),
        }
    }
}

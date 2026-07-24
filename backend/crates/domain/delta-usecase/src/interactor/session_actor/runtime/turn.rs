//! The turn state machine's runtime application: applying one input,
//! reading the current phase, and the turn-end / session-deletion clears.

use crate::turn::{transition, Transition, TurnInput, TurnState};

use super::SessionRuntime;

impl SessionRuntime {
    /// The session's current turn state.
    pub fn turn(&self) -> TurnState {
        self.turn
    }

    /// Apply one input to the turn state machine, returning the full
    /// transition (the caller executes the orphan disposition and logs
    /// anomalies). The transition table lives in the `turn` module.
    ///
    /// A transition back to [`TurnState::Idle`] (stop, interrupt, close) also
    /// drops any pending permission dialog and pending question: both blocked
    /// that turn, so the turn ending — however it ended — makes them moot. This
    /// is the same lifecycle the browser notices have.
    pub fn apply_turn(&mut self, input: TurnInput) -> Transition {
        let result = transition(self.turn, input);
        self.turn = result.next;
        if result.next == TurnState::Idle {
            self.pending_permission = None;
            self.pending_question = None;
            // A FOREGROUND subagent cannot outlive the turn that spawned it:
            // once the turn ends (stop, interrupt, close) any still-running
            // foreground entry is moot, so drop it. This also covers the case
            // where a foreground `PostToolUse(Agent)` was somehow missed — the
            // turn end clears it rather than leaving a stuck indicator. A
            // BACKGROUND subagent (`run_in_background: true`) deliberately
            // outlives the launching turn: it keeps running after the turn
            // returns to idle, so it is kept here and removed only when its
            // completion `<task-notification>` is folded.
            self.running_subagents.retain(|s| s.background);
            // The provisional live preview belongs to the turn that just ended;
            // the persisted assistant message (ingested by the transcript sync)
            // now renders instead, so drop the preview to avoid a duplicate.
            self.streaming_message = None;
        }
        result
    }

    /// Drop the turn state without any orphan handling. Used when the session
    /// row itself is being deleted (its sends go with it by cascade).
    ///
    /// Unlike [`Self::apply_turn`], this clears the WHOLE running set including
    /// background subagents: the session is being deleted, so no later
    /// completion notification can arrive to finish a background entry — keeping
    /// one would pin a doomed actor alive forever.
    pub fn forget_turn(&mut self) {
        self.turn = TurnState::Idle;
        self.pending_permission = None;
        self.pending_question = None;
        self.running_subagents.clear();
        self.streaming_message = None;
    }
}

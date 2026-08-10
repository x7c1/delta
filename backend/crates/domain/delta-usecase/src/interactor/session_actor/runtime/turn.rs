//! The turn state machine's runtime application: applying one input,
//! reading the current phase, and the turn-end / session-deletion clears.

use crate::turn::{transition, Transition, TurnInput, TurnState};

use super::SessionRuntime;

/// How many times one send may be returned to `queued` by the turn machine
/// before Delta stops re-dispatching it and parks it instead.
///
/// One retry, not zero: a send whose echo was mangled once (interleaved pane
/// typing, a compaction swallowing the keystrokes) really does come back
/// intact on the next attempt, and losing a composed message to a single
/// hiccup would be worse than the retry. Not more than one: a send whose echo
/// can *never* match — Claude Code rewrites the prompt, so equality is
/// unreachable — mismatches identically on every attempt, and each attempt
/// costs a full model turn. Two dispatches are enough to tell the two apart.
pub const MAX_REQUEUES_PER_SEND: u32 = 1;

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
        // A matched send has left the outstanding set for good; dropping its
        // budget here (not only on the orphan dispositions) keeps the map
        // bounded by the sends in flight, not by the session's lifetime.
        if let TurnInput::EchoMatched { send_id } = input {
            self.forget_requeues(send_id);
        }
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
        self.requeues_per_send.clear();
    }

    /// Spend one requeue from `send_id`'s budget, reporting whether the send
    /// may still be returned to `queued` (and so re-dispatched on the next
    /// idle). `false` means the budget is exhausted: the caller must park the
    /// send instead of requeueing it, or the dispatch⇄mismatch cycle never
    /// ends. See [`MAX_REQUEUES_PER_SEND`] and the `requeues_per_send` field
    /// docs for why the cap is where it is.
    pub fn claim_requeue(&mut self, send_id: i64) -> bool {
        let spent = self.requeues_per_send.entry(send_id).or_insert(0);
        *spent += 1;
        *spent <= MAX_REQUEUES_PER_SEND
    }

    /// Drop `send_id`'s requeue budget: the send left the outstanding set
    /// (matched, cancelled, or parked), so its retry history is moot.
    pub fn forget_requeues(&mut self, send_id: i64) {
        self.requeues_per_send.remove(&send_id);
    }
}

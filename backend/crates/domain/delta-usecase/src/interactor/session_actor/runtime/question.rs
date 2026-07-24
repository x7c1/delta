//! [`PendingQuestion`]: the `AskUserQuestion` tool call awaiting the user's
//! pick in the TUI.

use delta_model::ThreadId;

use super::SessionRuntime;

/// An `AskUserQuestion` tool call currently presenting its options in the TUI,
/// awaiting the user's pick.
///
/// The queryable counterpart of the `QuestionAsked` broadcast, mirroring
/// [`PendingPermission`]: the event is lost for a client whose socket was down
/// when it fired, so the sends envelope reports this state and a reconnecting
/// client rebuilds its question card from a plain refetch. Cleared when the
/// correlated `tool_result` resolves the request (the user answered in the TUI)
/// and whenever the turn returns to idle — a question cannot outlive its turn.
///
/// [`PendingPermission`]: super::PendingPermission
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingQuestion {
    /// The `PreToolUse` row id that recorded this question (its `tool_use_id`
    /// is what the later `tool_result` resolves it by).
    pub request_id: i64,
    /// The in-flight turn's thread, so the browser only shows the question card
    /// on the thread it belongs to.
    pub thread_id: ThreadId,
    /// The raw `{"questions":[…]}` tool input, serialized as JSON text, which
    /// the browser parses to render the question card.
    pub tool_input_json: String,
}

impl SessionRuntime {
    /// Record the `AskUserQuestion` now presenting its options in the TUI (a
    /// new question replaces a stale one — `claude` shows one at a time).
    pub fn set_pending_question(&mut self, pending: PendingQuestion) {
        self.pending_question = Some(pending);
    }

    /// The `AskUserQuestion` currently presenting its options in the TUI, if
    /// any. Read by the answer path to correlate an incoming answer by
    /// `request_id` and parse its question shapes for the key generator.
    pub fn pending_question(&self) -> Option<&PendingQuestion> {
        self.pending_question.as_ref()
    }

    /// Drop the pending question if `request_id` is the one it tracks. Keyed so
    /// a stale resolution can never wipe a newer question's state — the same
    /// guard [`Self::resolve_pending_permission`] applies.
    pub fn resolve_pending_question(&mut self, request_id: i64) {
        if self
            .pending_question
            .as_ref()
            .is_some_and(|q| q.request_id == request_id)
        {
            self.pending_question = None;
        }
    }
}

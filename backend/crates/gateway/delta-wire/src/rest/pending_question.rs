//! An `AskUserQuestion` tool call currently presenting its options.

use delta_usecase::PendingQuestion;
use serde::Serialize;
use ts_rs::TS;

/// An `AskUserQuestion` tool call currently presenting its options in the TUI,
/// as reported on the REST surface.
///
/// The queryable counterpart of the `question_asked` event (same fields, minus
/// the session id the URL already names): the event is lost for a client whose
/// socket was down when it fired, so a reconnecting client rebuilds its
/// question card from this instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PendingQuestion")]
pub struct WirePendingQuestion {
    /// The `PreToolUse` row id whose `tool_result` resolves this question.
    pub request_id: i64,
    /// The in-flight turn's thread, so a reconnecting client shows the question
    /// card only on the thread it belongs to.
    pub thread_id: i64,
    /// The raw `{"questions":[…]}` tool input, serialized as JSON text.
    pub tool_input: String,
}

impl From<PendingQuestion> for WirePendingQuestion {
    fn from(pending: PendingQuestion) -> Self {
        WirePendingQuestion {
            request_id: pending.request_id,
            thread_id: pending.thread_id.0,
            tool_input: pending.tool_input_json,
        }
    }
}

//! Request body for `POST /api/sessions/{id}/questions/cancel`.

use serde::Deserialize;
use ts_rs::TS;

/// The browser's request to cancel a pending `AskUserQuestion`.
///
/// Unlike an answer (which carries the chosen option indices), a cancel has no
/// selection to send — the only datum is which question to cancel — so the
/// `request_id` rides in the body rather than the path. The server injects a
/// single `Escape` into the session's pane, which cancels the whole call; the
/// TUI then writes an `is_error` `tool_result` whose flush clears the card
/// through the normal resolution path. A `409` reply means the question is no
/// longer pending (already answered/cancelled, its turn ended, or no live pane),
/// in which case the browser falls back to cancelling in the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "QuestionCancelRequest")]
pub struct WireQuestionCancelRequest {
    pub request_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_the_request_id() {
        let body: WireQuestionCancelRequest =
            serde_json::from_str(r#"{ "request_id": 7 }"#).unwrap();
        assert_eq!(body.request_id, 7);
    }
}

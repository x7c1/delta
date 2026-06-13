//! Request body for `POST /api/sessions/{id}/questions/{request_id}/answer`.

use serde::Deserialize;
use ts_rs::TS;

/// The browser's answer to a pending `AskUserQuestion`: the chosen option
/// index(es) for each question, in question order.
///
/// `selections[q]` lists the 0-based option indices selected for question `q` —
/// exactly one for a single-select question, one or more for a multi-select
/// one. The server turns these into the exact TUI keystrokes (the pinned
/// key-sequence generator) and injects them into the session's pane; a `409`
/// reply means the question is no longer pending (already answered, or its turn
/// ended) and a `400` that the selection was malformed, in either case the
/// browser falls back to answering in the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "QuestionAnswerRequest")]
pub struct WireQuestionAnswerRequest {
    pub selections: Vec<Vec<u32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_per_question_selection_groups() {
        let body: WireQuestionAnswerRequest =
            serde_json::from_str(r#"{ "selections": [[0], [2, 1]] }"#).unwrap();
        assert_eq!(body.selections, vec![vec![0], vec![2, 1]]);
    }

    #[test]
    fn deserializes_an_empty_selection_list() {
        let body: WireQuestionAnswerRequest =
            serde_json::from_str(r#"{ "selections": [] }"#).unwrap();
        assert_eq!(body.selections, Vec::<Vec<u32>>::new());
    }
}

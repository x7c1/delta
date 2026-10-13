//! `/api/sessions/{id}/questions` — answering or cancelling a pending question.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_usecase::SessionId;
use delta_wire::rest::{WireQuestionAnswerRequest, WireQuestionCancelRequest};

use crate::state::AppState;

use super::ApiError;

/// `POST /api/sessions/{id}/questions/{request_id}/answer` — answer a pending
/// `AskUserQuestion` from the browser.
///
/// A CLI hook cannot return the user's pick, so the server turns the per-question
/// selected option indices into the exact TUI keystrokes (the pinned
/// key-sequence generator) and injects them into the session's live pane. The
/// TUI then records the answer and the turn proceeds; the eventual `tool_result`
/// resolves the question's request row through the normal sync, which clears the
/// card via the same `permission_resolved` path a terminal-answered question
/// takes — so no event is broadcast here.
///
/// Replies `409` when the question is no longer pending (already answered, its
/// turn ended, or no live pane) and `400` for a malformed selection; the browser
/// then falls back to the answer-in-the-terminal guidance.
pub(crate) async fn answer_question(
    State(state): State<AppState>,
    Path((id, request_id)): Path<(String, i64)>,
    Json(req): Json<WireQuestionAnswerRequest>,
) -> Result<StatusCode, ApiError> {
    // The wire form uses `u32` indices (non-negative on the wire); widen to the
    // `usize` the domain generator indexes options with.
    let selections: Vec<Vec<usize>> = req
        .selections
        .into_iter()
        .map(|group| group.into_iter().map(|index| index as usize).collect())
        .collect();
    state
        .interactor()
        .answer_question(&SessionId::from(id), request_id, selections)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/questions/cancel` — cancel a pending
/// `AskUserQuestion` from the browser.
///
/// The sibling of [`answer_question`]: a CLI hook cannot cancel the question, so
/// the server injects a single `Escape` into the session's live pane, which
/// cancels the whole call. The TUI then writes an `is_error` `tool_result`, and
/// that flush resolves the question's request row through the normal sync, which
/// clears the card via the same `permission_resolved` path a terminal-cancelled
/// question takes — so no event is broadcast here.
///
/// Unlike an answer, cancel carries no selection, so the `request_id` rides in
/// the body rather than the path. Replies `409` when the question is no longer
/// pending (already answered/cancelled, its turn ended, or no live pane); the
/// browser then falls back to the cancel-in-the-terminal guidance. There is no
/// `400` case — there is no selection to malform.
pub(crate) async fn cancel_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<WireQuestionCancelRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .interactor()
        .cancel_question(&SessionId::from(id), req.request_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

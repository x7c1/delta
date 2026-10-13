//! `POST /api/permissions/{id}/decision` — answering a tool-permission request.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::WirePermissionDecisionRequest;

use crate::state::AppState;

use super::ApiError;

/// `POST /api/permissions/{id}/decision` — answer a pending tool-permission
/// request from the browser.
///
/// Resolves the request row and wakes the blocked `PermissionRequest` hook
/// response, which carries the decision back to Claude Code — so the tool
/// proceeds (or is denied) without anyone touching the TUI prompt. Replies
/// `409` when the request is no longer awaiting a browser decision (already
/// decided, or its hook wait timed out and the TUI prompt owns it now); the
/// browser then falls back to guidance chosen by the provider's `has_terminal`
/// capability.
pub(crate) async fn decide_permission(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WirePermissionDecisionRequest>,
) -> Result<StatusCode, ApiError> {
    let events = state
        .interactor()
        .decide_permission(id, req.decision.into())
        .await?;
    state.broadcast(events);
    Ok(StatusCode::NO_CONTENT)
}

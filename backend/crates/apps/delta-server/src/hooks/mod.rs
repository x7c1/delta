//! Claude Code HTTP hook handlers.
//!
//! Claude Code fires these hooks even inside an interactive tmux session, so
//! they form Delta's control plane:
//!
//! - `UserPromptSubmit` fires just before a prompt is processed. Delta matches
//!   it against the pending-send FIFO to confirm a turn start, and may return a
//!   `hookSpecificOutput.additionalContext` to inject a locator quote into that
//!   prompt only.
//! - `Stop` fires when a response completes.
//! - `PreToolUse` fires for every tool call; Delta only records the request
//!   (it carries the `tool_use_id` needed to resolve the notice later) and does
//!   not notify the browser — the TUI decides allow/deny.
//! - `PermissionRequest` fires only when an interactive permission dialog
//!   actually appears (a human answer is genuinely pending); Delta notifies the
//!   browser, correlating it to the request recorded at `PreToolUse`.
//! - `SessionEnd` fires when a session terminates. Delta uses it as a precise
//!   early failure signal: if the ending session is a fresh spawn that never
//!   bound, the launch failed, so Delta reports `SpawnFailed`; an already-bound
//!   session ending is a normal end and changes nothing.

mod permission_request_payload;
pub use permission_request_payload::PermissionRequestPayload;
mod pre_tool_use_payload;
pub use pre_tool_use_payload::PreToolUsePayload;
mod session_end_payload;
pub use session_end_payload::SessionEndPayload;
mod session_start_payload;
pub use session_start_payload::SessionStartPayload;
mod stop_payload;
pub use stop_payload::StopPayload;
mod user_prompt_submit_payload;
pub use user_prompt_submit_payload::UserPromptSubmitPayload;
mod user_prompt_submit_response;
pub use user_prompt_submit_response::UserPromptSubmitResponse;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use delta_usecase::{
    SessionEndHook, SessionId, SessionStartHook, StopHook, UserPromptSubmitHook,
};

use crate::state::AppState;

/// Map a use-case error to a 500 with a logged reason.
fn internal_error(err: delta_usecase::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "hook handler failed");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

pub async fn user_prompt_submit(
    State(state): State<AppState>,
    Json(payload): Json<UserPromptSubmitPayload>,
) -> impl IntoResponse {
    let hook = UserPromptSubmitHook {
        prompt: payload.prompt,
        session_id: SessionId::from(payload.session_id),
        transcript_path: payload.transcript_path,
        cwd: payload.cwd,
    };

    match state.interactor().on_user_prompt_submit(hook).await {
        Ok((events, additional_context)) => {
            tracing::info!(
                additional_context = additional_context.as_deref().unwrap_or("<none>"),
                "UserPromptSubmit: additionalContext returned to Claude Code"
            );
            state.broadcast(events);
            match additional_context {
                // Only emit a body when there is a locator quote to inject;
                // Claude Code consumes it solely from the `hookSpecificOutput`
                // envelope. With nothing to inject, a plain 200 is enough.
                Some(context) => Json(UserPromptSubmitResponse::inject(context)).into_response(),
                None => StatusCode::OK.into_response(),
            }
        }
        Err(err) => internal_error(err).into_response(),
    }
}

pub async fn stop(
    State(state): State<AppState>,
    Json(payload): Json<StopPayload>,
) -> impl IntoResponse {
    let hook = StopHook {
        session_id: SessionId::from(payload.session_id),
        stop_reason: payload.stop_reason,
    };

    match state.interactor().on_stop(hook).await {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

/// Handle a `PermissionRequest` hook: an interactive permission dialog has
/// appeared, so a human answer is genuinely pending. Correlate it to the request
/// recorded at `PreToolUse` and broadcast the resulting `PermissionRequested` so
/// the browser shows the notice.
pub async fn permission_request(
    State(state): State<AppState>,
    Json(payload): Json<PermissionRequestPayload>,
) -> impl IntoResponse {
    let session_id = SessionId::from(payload.session_id);
    let tool_input_json = payload.tool_input.to_string();
    match state
        .interactor()
        .on_permission_request(&session_id, &payload.tool_name, &tool_input_json)
        .await
    {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

/// Handle a `SessionEnd` hook: a session has terminated. When that session is a
/// fresh spawn that never bound, the launch failed before it could register, so
/// the use case removes it and emits `SpawnFailed`; an already-bound session
/// ending is a normal end and emits nothing.
pub async fn session_end(
    State(state): State<AppState>,
    Json(payload): Json<SessionEndPayload>,
) -> impl IntoResponse {
    let hook = SessionEndHook {
        session_id: SessionId::from(payload.session_id),
        reason: payload.reason,
    };

    match state.interactor().on_session_end(hook).await {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

/// Handle a `SessionStart` hook: the session's TUI is ready to accept input.
/// On `source=startup` it binds and registers a matching fresh spawn (even a
/// prompt-less one); on `source=resume` it releases the held first prompt of a
/// resumed session; `clear`/`compact` are safe no-ops. Broadcasts whatever
/// events the bind produced (typically `SessionRegistered`).
pub async fn session_start(
    State(state): State<AppState>,
    Json(payload): Json<SessionStartPayload>,
) -> impl IntoResponse {
    let hook = SessionStartHook {
        session_id: SessionId::from(payload.session_id),
        source: payload.source,
        cwd: payload.cwd,
        transcript_path: payload.transcript_path,
    };

    match state.interactor().on_session_start(hook).await {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

pub async fn pre_tool_use(
    State(state): State<AppState>,
    Json(payload): Json<PreToolUsePayload>,
) -> impl IntoResponse {
    let session_id = SessionId::from(payload.session_id);
    let tool_input_json = payload.tool_input.to_string();

    match state
        .interactor()
        .on_pre_tool_use(
            &session_id,
            &payload.tool_name,
            &tool_input_json,
            &payload.tool_use_id,
        )
        .await
    {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

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
//! - `MessageDisplay` fires repeatedly while a response is being generated,
//!   before the transcript is flushed. Delta buffers each visible text chunk as
//!   a provisional live preview of the in-flight turn and broadcasts it to the
//!   browser; the hook is passive (an empty 200) so it never mutates the TUI.
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
//!
//! The payload shapes live in `delta_wire::hooks`; the handlers here convert
//! them into the domain port types and broadcast the resulting events.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use delta_usecase::{
    MessageDisplayHook, PermissionDecision, SessionEndHook, SessionId, SessionStartHook, StopHook,
    UserPromptSubmitHook,
};
use delta_wire::hooks::{
    MessageDisplayPayload, PermissionRequestPayload, PermissionRequestResponse, PreToolUsePayload,
    SessionEndPayload, SessionStartPayload, StopPayload, UserPromptSubmitPayload,
    UserPromptSubmitResponse,
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

/// Handle a `MessageDisplay` hook: one chunk of the in-flight turn's assistant
/// message, streamed live before the transcript is flushed. Delta buffers it as
/// a provisional preview and broadcasts an `AssistantStreaming` event for the
/// browser, then answers an empty `200` so the hook stays passive and never
/// mutates the TUI display.
pub async fn message_display(
    State(state): State<AppState>,
    Json(payload): Json<MessageDisplayPayload>,
) -> impl IntoResponse {
    let hook = MessageDisplayHook {
        session_id: SessionId::from(payload.session_id),
        message_id: payload.message_id,
        index: payload.index,
        final_: payload.r#final,
        delta: payload.delta,
    };

    match state.interactor().on_message_display(hook).await {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

/// Handle a `PermissionRequest` hook: an interactive permission dialog has
/// appeared, so a human answer is genuinely pending.
///
/// The use case records the request row and registers a decision waiter; the
/// resulting `PermissionRequested` is broadcast *before* blocking so the
/// browser can show the Allow/Deny notice it is being asked to answer. The
/// response then blocks Claude Code until either:
///
/// - the browser decides (`POST /api/permissions/{id}/decision`), in which
///   case the body carries `hookSpecificOutput.decision.behavior`
///   (`allow`/`deny`) and Claude Code acts on it without a TUI prompt; or
/// - the deadline passes, in which case the waiter is abandoned and the
///   response is an empty `200` — no decision to report — so the tool call
///   falls back to the interactive TUI prompt exactly as it would without
///   this hook. The row stays `pending` and the eventual `tool_result`
///   resolves it.
pub async fn permission_request(
    State(state): State<AppState>,
    Json(payload): Json<PermissionRequestPayload>,
) -> impl IntoResponse {
    let session_id = SessionId::from(payload.session_id);
    let tool_input_json = payload.tool_input.to_string();
    let wait = match state
        .interactor()
        .on_permission_request(&session_id, &payload.tool_name, &tool_input_json)
        .await
    {
        Ok(wait) => wait,
        Err(err) => return internal_error(err).into_response(),
    };
    state.broadcast(wait.events);

    let deadline = state.interactor().permission_decision_deadline();
    match tokio::time::timeout(deadline, wait.decision).await {
        Ok(Ok(decision)) => {
            tracing::info!(
                request_id = wait.request_id,
                ?decision,
                "PermissionRequest: browser decision returned to Claude Code"
            );
            Json(PermissionRequestResponse::decided(
                decision == PermissionDecision::Allow,
            ))
            .into_response()
        }
        // Timed out, or the waiter was dropped: no decision to report. The
        // empty 200 is the deliberate passthrough — Claude Code continues
        // through its normal interactive permission flow.
        Ok(Err(_)) | Err(_) => {
            state
                .interactor()
                .abandon_permission_decision(wait.request_id)
                .await;
            tracing::info!(
                request_id = wait.request_id,
                "PermissionRequest: no browser decision before the deadline; \
                 passing through to the TUI prompt"
            );
            StatusCode::OK.into_response()
        }
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

//! Claude Code HTTP hook handlers.
//!
//! Claude Code fires these hooks even inside an interactive tmux session, so
//! they form Delta's control plane:
//!
//! - `UserPromptSubmit` fires just before a prompt is processed. Delta matches
//!   it against the pending-send FIFO to confirm a turn start, and may return
//!   `additionalContext` to inject a locator quote into that prompt only.
//! - `Stop` fires when a response completes.
//! - `PreToolUse` fires when a permission prompt is imminent; Delta only
//!   notifies the browser and records the request — the TUI decides allow/deny.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use delta_usecase::{SessionId, StopHook, UserPromptSubmitHook};

use crate::state::AppState;

/// `UserPromptSubmit` payload.
#[derive(Debug, Deserialize)]
pub struct UserPromptSubmitPayload {
    pub prompt: String,
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
}

/// `UserPromptSubmit` response. When present, `additional_context` is injected
/// into this prompt only.
#[derive(Debug, Default, Serialize)]
pub struct UserPromptSubmitResponse {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// `Stop` payload.
#[derive(Debug, Deserialize)]
pub struct StopPayload {
    pub session_id: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}

/// `PreToolUse` payload.
#[derive(Debug, Deserialize)]
pub struct PreToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
}

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
            state.broadcast(events);
            Json(UserPromptSubmitResponse { additional_context }).into_response()
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
        last_assistant_message: payload.last_assistant_message,
    };

    match state.interactor().on_stop(hook).await {
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
        .on_pre_tool_use(&session_id, &payload.tool_name, &tool_input_json)
        .await
    {
        Ok(events) => {
            state.broadcast(events);
            StatusCode::OK.into_response()
        }
        Err(err) => internal_error(err).into_response(),
    }
}

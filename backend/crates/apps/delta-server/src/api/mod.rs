//! Browser REST surface.
//!
//! The browser talks to the server over a REST + WebSocket hybrid: queries and
//! commands go through these `/api/*` routes (so they are easy to mock on the
//! frontend), live deltas arrive over `/ws`, and the terminal is bridged over
//! `/pty`. Every handler maps onto the use-case [`Interactor`]; errors are
//! converted to HTTP responses through a single [`ApiError`] mapping.
//!
//! [`Interactor`]: delta_usecase::Interactor

mod api_error;
pub(crate) use api_error::ApiError;
mod create_send_request;
pub use create_send_request::CreateSendRequest;
mod create_send_response;
pub use create_send_response::CreateSendResponse;
mod error_body;
mod messages_response;
pub use messages_response::MessagesResponse;
mod session_response;
pub use session_response::SessionResponse;
mod threads_response;
pub use threads_response::ThreadsResponse;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;

use delta_usecase::ThreadId;

use crate::state::AppState;

/// `GET /api/session` — the current session for hydration.
pub(crate) async fn get_session(State(state): State<AppState>) -> Result<Response, ApiError> {
    use axum::response::IntoResponse;
    match state.interactor().current_session().await? {
        Some((session, main_thread_id)) => Ok(Json(SessionResponse {
            session,
            main_thread_id,
        })
        .into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

/// `GET /api/threads` — the thread tree for the navigator.
pub(crate) async fn list_threads(
    State(state): State<AppState>,
) -> Result<Json<ThreadsResponse>, ApiError> {
    let threads = state.interactor().threads().await?;
    Ok(Json(ThreadsResponse { threads }))
}

/// `GET /api/threads/{id}/messages` — a thread's messages for drill-down.
pub(crate) async fn thread_messages(
    State(state): State<AppState>,
    Path(thread_id): Path<i64>,
) -> Result<Json<MessagesResponse>, ApiError> {
    let messages = state.interactor().thread_view(ThreadId(thread_id)).await?;
    Ok(Json(MessagesResponse { messages }))
}

/// `POST /api/sends` — enqueue a send (a branch send when a semantic parent is
/// given). No event is broadcast here; turn confirmation arrives later via the
/// `UserPromptSubmit` hook.
pub(crate) async fn create_send(
    State(state): State<AppState>,
    Json(req): Json<CreateSendRequest>,
) -> Result<(StatusCode, Json<CreateSendResponse>), ApiError> {
    let send = state
        .interactor()
        .enqueue_send(
            req.thread_id,
            &req.text,
            req.locator_quote.as_deref(),
            req.semantic_parent_uuid.as_ref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(CreateSendResponse { send })))
}

//! Browser REST surface.
//!
//! The browser talks to the server over a REST + WebSocket hybrid: queries and
//! commands go through these `/api/*` routes (so they are easy to mock on the
//! frontend), live deltas arrive over `/ws`, and the terminal is bridged over
//! `/pty`. Every handler maps onto the use-case [`Interactor`]; errors are
//! converted to HTTP responses through a single [`ApiError`] mapping.
//!
//! The surface is multi-session: sessions are listed, created, opened, and
//! closed by id, and threads and sends are routed to a specific session rather
//! than an implicit "current" one.
//!
//! [`Interactor`]: delta_usecase::Interactor

mod api_error;
pub(crate) use api_error::ApiError;
mod create_send_request;
pub use create_send_request::CreateSendRequest;
mod create_send_response;
pub use create_send_response::CreateSendResponse;
mod ensure_session_response;
pub use ensure_session_response::EnsureSessionResponse;
mod error_body;
mod messages_response;
pub use messages_response::MessagesResponse;
mod sessions_response;
pub use sessions_response::{SessionListItem, SessionsResponse};
mod threads_response;
pub use threads_response::ThreadsResponse;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_usecase::{SessionId, ThreadId};

use crate::state::AppState;

/// `GET /api/sessions` — every known session, annotated with its live state.
///
/// Lists all stored sessions (ordered by creation), each tagged with whether it
/// currently has a live pane (`open`) and its `main` thread id, so the navigator
/// can show and route into every conversation — open or closed.
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<SessionsResponse>, ApiError> {
    let sessions = state.interactor().list_sessions().await?;
    Ok(Json(SessionsResponse {
        sessions: sessions.into_iter().map(SessionListItem::from).collect(),
    }))
}

/// `POST /api/sessions` — spawn a fresh session eagerly.
///
/// Used by cold start (an empty session list) and the "New" button. Returns the
/// tmux/process lifecycle so the UI can show a "starting" indicator until the
/// session is usable. The conversational session is still registered later by
/// the first `UserPromptSubmit` hook, so a freshly created session has no
/// `Session` row yet (it appears in `GET /api/sessions` once registered).
pub(crate) async fn create_session(
    State(state): State<AppState>,
) -> Result<Json<EnsureSessionResponse>, ApiError> {
    let status = state.ensure_session().await?;
    Ok(Json(EnsureSessionResponse {
        status: status.into(),
    }))
}

/// `POST /api/sessions/{id}/open` — resume a closed, known session.
///
/// Re-launches `claude --resume <id>` and binds the new pane, broadcasting
/// `SessionOpened`. Re-opening an already-open session is a no-op.
pub(crate) async fn open_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    state.interactor().open_session(&id).await?;
    state.broadcast([delta_usecase::SessionEvent::SessionOpened { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/close` — tear down a session's pane, keep its data.
///
/// Kills the live pane and drops it from the registry, broadcasting
/// `SessionClosed`; the conversation remains in the store and can be reopened.
pub(crate) async fn close_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    state.interactor().close_session(&id).await?;
    state.broadcast([delta_usecase::SessionEvent::SessionClosed { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/sessions/{id}/threads` — a session's thread tree for the navigator.
pub(crate) async fn list_threads(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ThreadsResponse>, ApiError> {
    let threads = state.interactor().threads_for(&SessionId::from(id)).await?;
    Ok(Json(ThreadsResponse { threads }))
}

/// `GET /api/threads/{id}/messages` — a thread's messages for drill-down.
///
/// Thread ids are globally unique, so this is not scoped by session.
pub(crate) async fn thread_messages(
    State(state): State<AppState>,
    Path(thread_id): Path<i64>,
) -> Result<Json<MessagesResponse>, ApiError> {
    let messages = state.interactor().thread_view(ThreadId(thread_id)).await?;
    Ok(Json(MessagesResponse { messages }))
}

/// `POST /api/sends` — enqueue a send into a session named by the request.
///
/// The session is derived from the target thread for an existing send, or
/// created for a `new_session` send. No event is broadcast here; turn
/// confirmation arrives later via the `UserPromptSubmit` hook.
pub(crate) async fn create_send(
    State(state): State<AppState>,
    Json(req): Json<CreateSendRequest>,
) -> Result<(StatusCode, Json<CreateSendResponse>), ApiError> {
    let (target, text, locator_quote) = req
        .into_target()
        .map_err(|err| ApiError::BadRequest(err.message().to_owned()))?;
    let send = state
        .interactor()
        .enqueue_send(target, &text, locator_quote.as_deref())
        .await?;
    Ok((StatusCode::CREATED, Json(CreateSendResponse { send })))
}

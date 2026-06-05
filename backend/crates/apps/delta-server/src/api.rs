//! Browser REST surface.
//!
//! The browser talks to the server over a REST + WebSocket hybrid: queries and
//! commands go through these `/api/*` routes (so they are easy to mock on the
//! frontend), live deltas arrive over `/ws`, and the terminal is bridged over
//! `/pty`. Every handler maps onto the use-case [`Interactor`]; errors are
//! converted to HTTP responses through a single [`ApiError`] mapping.
//!
//! [`Interactor`]: delta_usecase::Interactor

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use delta_usecase::{Message, MessageUuid, PendingSend, Session, Thread, ThreadId};

use crate::state::AppState;

/// A use-case error rendered as an HTTP response.
///
/// This is the single place that maps [`delta_usecase::Error`] onto status
/// codes, keeping the handlers free of ad-hoc error handling.
pub(crate) struct ApiError(delta_usecase::Error);

impl From<delta_usecase::Error> for ApiError {
    fn from(err: delta_usecase::Error) -> Self {
        ApiError(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use delta_usecase::Error;
        let status = match &self.0 {
            // No session yet means nothing to act on for the caller.
            Error::NoSession => StatusCode::NOT_FOUND,
            Error::ThreadNotFound(_) => StatusCode::NOT_FOUND,
            // Bad domain values coming over the wire are the caller's fault.
            Error::Model(_) => StatusCode::BAD_REQUEST,
            // Everything else is an internal failure.
            Error::Tmux(_) | Error::Transcript(_) | Error::Store(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        if status.is_server_error() {
            tracing::error!(error = %self.0, "api handler failed");
        }
        let body = Json(ErrorBody {
            error: self.0.to_string(),
        });
        (status, body).into_response()
    }
}

/// The JSON body returned for any error response.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// Response for `GET /api/session`.
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub session: Session,
    pub main_thread_id: ThreadId,
}

/// Response for `GET /api/threads`.
#[derive(Debug, Serialize)]
pub struct ThreadsResponse {
    pub threads: Vec<Thread>,
}

/// Response for `GET /api/threads/{id}/messages`.
#[derive(Debug, Serialize)]
pub struct MessagesResponse {
    pub messages: Vec<Message>,
}

/// Request body for `POST /api/sends`.
#[derive(Debug, Deserialize)]
pub struct CreateSendRequest {
    /// The thread to send into (typically `main`). When `semantic_parent_uuid`
    /// is set this is the parent thread the new branch is created off.
    pub thread_id: ThreadId,
    /// When present, this is a branch send: the Interactor creates an unnamed
    /// child thread off this message and attributes the send to it.
    #[serde(default)]
    pub semantic_parent_uuid: Option<MessageUuid>,
    /// The text to send into the session.
    pub text: String,
    /// An optional quote to inject as `additionalContext` on the matched turn.
    #[serde(default)]
    pub locator_quote: Option<String>,
}

/// Response for `POST /api/sends`: the queued send, including the thread it was
/// attributed to (a freshly created child thread for a branch send).
#[derive(Debug, Serialize)]
pub struct CreateSendResponse {
    pub send: PendingSend,
}

/// `GET /api/session` — the current session for hydration.
pub(crate) async fn get_session(State(state): State<AppState>) -> Result<Response, ApiError> {
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
pub(crate) async fn list_threads(State(state): State<AppState>) -> Result<Json<ThreadsResponse>, ApiError> {
    let threads = state.interactor().threads().await?;
    Ok(Json(ThreadsResponse { threads }))
}

/// `GET /api/threads/{id}/messages` — a thread's trunk for drill-down.
pub(crate) async fn thread_messages(
    State(state): State<AppState>,
    Path(thread_id): Path<i64>,
) -> Result<Json<MessagesResponse>, ApiError> {
    let messages = state.interactor().thread_view(ThreadId(thread_id)).await?;
    Ok(Json(MessagesResponse { messages }))
}

/// `POST /api/sends` — enqueue a send (a branch send when a semantic parent is
/// given), broadcasting nothing here: turn confirmation arrives via the hook.
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

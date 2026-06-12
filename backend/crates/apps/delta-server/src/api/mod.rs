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
//! The JSON shapes themselves live in the `delta_wire` crate (its [`rest`]
//! module), which also generates the frontend's TypeScript bindings. Handlers
//! convert at this boundary: domain values in and out of the use cases, wire
//! types on the HTTP surface.
//!
//! [`Interactor`]: delta_usecase::Interactor
//! [`rest`]: delta_wire::rest

mod api_error;
pub(crate) use api_error::ApiError;
mod session_cursor;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use delta_usecase::{SessionId, ThreadId};
use delta_wire::rest::{
    WireCreateSendRequest, WireMessagesResponse, WireNewSessionResponse,
    WirePermissionDecisionRequest, WireRecentWorkdirItem, WireSendResponse, WireSendsResponse,
    WireSessionListItem, WireSessionsResponse, WireThreadsResponse, WireWorkdirListResponse,
    WireWorkdirRecentResponse,
};

use crate::state::AppState;

/// The default page size when the request omits `limit`.
const DEFAULT_PAGE_LIMIT: u32 = 30;

/// The hard cap on page size, so a caller cannot ask for an unbounded page.
const MAX_PAGE_LIMIT: u32 = 100;

/// Query parameters for `GET /api/sessions`: the opaque page cursor and an
/// optional page-size override. Mirrors the local-struct convention used by the
/// PTY bridge's `PtyQuery`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListSessionsQuery {
    /// The `next_cursor` echoed back from the previous page, or absent for the
    /// first page. Opaque: encoded/decoded by [`session_cursor`].
    cursor: Option<String>,
    /// Requested page size, clamped to `[1, MAX_PAGE_LIMIT]`; defaults to
    /// `DEFAULT_PAGE_LIMIT` when absent.
    limit: Option<u32>,
}

/// `GET /api/sessions` — one page of known sessions, most-recently-active first.
///
/// Returns a single page (most-recently-active first), each session tagged with
/// whether it currently has a live pane (`open`) and its `main` thread id, so
/// the navigator can show and route into every conversation — open or closed.
/// The page size is `limit` (default [`DEFAULT_PAGE_LIMIT`], capped at
/// [`MAX_PAGE_LIMIT`]). When more rows may follow, `next_cursor` carries an
/// opaque token the caller echoes back as `cursor` to fetch the next page; a
/// malformed `cursor` is a `400`.
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<WireSessionsResponse>, ApiError> {
    let cursor = match query.cursor {
        Some(token) => Some(
            session_cursor::decode(&token)
                .ok_or_else(|| ApiError::BadRequest("malformed cursor".to_owned()))?,
        ),
        None => None,
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);

    let page = state.interactor().list_sessions_page(cursor, limit).await?;
    Ok(Json(WireSessionsResponse {
        sessions: page
            .listings
            .into_iter()
            .map(WireSessionListItem::from)
            .collect(),
        next_cursor: page.next.as_ref().map(session_cursor::encode),
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
) -> Result<Json<WireNewSessionResponse>, ApiError> {
    let status = state.ensure_session().await?;
    Ok(Json(WireNewSessionResponse::from(status)))
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
) -> Result<Json<WireThreadsResponse>, ApiError> {
    let threads = state.interactor().threads_for(&SessionId::from(id)).await?;
    Ok(Json(WireThreadsResponse::from(threads)))
}

/// `GET /api/sessions/{id}/sends` — a session's open (non-terminal) sends.
///
/// Returns the sends still in flight for the session — status `queued`
/// (held back until the session goes idle) or `dispatched` (typed into the
/// pane, awaiting transcript correlation) — oldest first. This is the source
/// of truth for the browser's pending-send strip. An unknown session id is a
/// `404`, so a reaped spawn is distinguishable from "nothing pending".
pub(crate) async fn list_sends(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WireSendsResponse>, ApiError> {
    let id = SessionId::from(id);
    let sends = state.interactor().open_sends_for(&id).await?;
    // The queryable live state (turn phase + pending permission dialog) rides
    // along so a reconnecting client can rebuild its in-progress indicator and
    // its permission notice from this one refetch (events broadcast while the
    // socket was down are not replayed).
    let live = state.interactor().live_state_for(&id).await;
    Ok(Json(WireSendsResponse::new(sends, live)))
}

/// `GET /api/threads/{id}/messages` — a thread's messages for drill-down.
///
/// Thread ids are globally unique, so this is not scoped by session.
pub(crate) async fn thread_messages(
    State(state): State<AppState>,
    Path(thread_id): Path<i64>,
) -> Result<Json<WireMessagesResponse>, ApiError> {
    let messages = state.interactor().thread_view(ThreadId(thread_id)).await?;
    Ok(Json(WireMessagesResponse::from(messages)))
}

/// Query parameters for `GET /api/workdir/list`: the directory to browse.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirListQuery {
    /// The absolute path to list. Omitted or empty defaults to the user's home
    /// directory, so the picker has a sensible starting point.
    #[serde(default)]
    path: Option<String>,
}

/// `GET /api/workdir/list` — browse a directory for the working-directory picker.
///
/// Lists the immediate subdirectories of `path` (dirs only, dot-directories
/// hidden, sorted by name), along with the canonical path and its parent so the
/// picker can step up. `path` defaults to `$HOME` when omitted. A missing path
/// or a non-directory is a `400`; a permission error is a `403`.
pub(crate) async fn list_workdir(
    State(state): State<AppState>,
    Query(query): Query<WorkdirListQuery>,
) -> Result<Json<WireWorkdirListResponse>, ApiError> {
    let listing = state
        .interactor()
        .browse_workdir(query.path.as_deref())
        .await?;
    Ok(Json(WireWorkdirListResponse::from(listing)))
}

/// `GET /api/workdir/recent` — recently-used working directories for the picker.
///
/// Returns the distinct directories sessions have run in, most-recently-used
/// first, derived from existing session rows (Delta keeps no separate history).
pub(crate) async fn recent_workdir(
    State(state): State<AppState>,
) -> Result<Json<WireWorkdirRecentResponse>, ApiError> {
    let workdirs = state.interactor().recent_workdirs().await?;
    Ok(Json(WireWorkdirRecentResponse {
        workdirs: workdirs
            .into_iter()
            .map(WireRecentWorkdirItem::from)
            .collect(),
    }))
}

/// `POST /api/sends` — enqueue a send into a session named by the request.
///
/// The session is derived from the target thread for an existing send, or
/// created for a `new_session` send. Turn confirmation arrives later via the
/// `UserPromptSubmit` hook; only enqueue-time events (e.g. `send_dispatched`
/// from the idle-flush) are broadcast here.
pub(crate) async fn create_send(
    State(state): State<AppState>,
    Json(req): Json<WireCreateSendRequest>,
) -> Result<(StatusCode, Json<WireSendResponse>), ApiError> {
    let (target, text, locator_quote) = req
        .into_target()
        .map_err(|err| ApiError::BadRequest(err.message().to_owned()))?;
    let (send, events) = state
        .interactor()
        .enqueue_send(target, &text, locator_quote.as_deref())
        .await?;
    // The enqueue may have promoted a previously-queued send (the idle-flush
    // safety net); broadcast so the browser sees the queued->dispatched
    // transition immediately.
    state.broadcast(events);
    Ok((StatusCode::CREATED, Json(WireSendResponse::from(send))))
}

/// `POST /api/permissions/{id}/decision` — answer a pending tool-permission
/// request from the browser.
///
/// Resolves the request row and wakes the blocked `PermissionRequest` hook
/// response, which carries the decision back to Claude Code — so the tool
/// proceeds (or is denied) without anyone touching the TUI prompt. Replies
/// `409` when the request is no longer awaiting a browser decision (already
/// decided, or its hook wait timed out and the TUI prompt owns it now); the
/// browser then falls back to the answer-in-the-terminal guidance.
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

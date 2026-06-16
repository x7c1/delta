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
    WireCreateLaunchOptionRequest, WireCreateSendRequest, WireGitBranchesResponse,
    WireGitRepoResponse, WireLaunchOption, WireLaunchOptionsResponse, WireMessagesResponse,
    WireNewSessionResponse, WirePermissionDecisionRequest, WireQuestionAnswerRequest,
    WireQuestionCancelRequest, WireRecentWorkdirItem, WireSendResponse, WireSendsResponse,
    WireSessionListItem, WireSessionsResponse, WireThreadsResponse, WireUpdateLaunchOptionRequest,
    WireWorkdirListResponse, WireWorkdirRecentResponse,
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
/// of truth for the browser's send strip. An unknown session id is a
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

/// Query parameters for the git-detection endpoints: the directory to inspect.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirGitQuery {
    /// The absolute path to inspect. Required: unlike the browse endpoints there
    /// is no sensible default repository to fall back to.
    #[serde(default)]
    path: Option<String>,
}

impl WorkdirGitQuery {
    /// The required `path`, or a `400` when it is missing or blank.
    fn require_path(&self) -> Result<&str, ApiError> {
        match self.path.as_deref() {
            Some(path) if !path.is_empty() => Ok(path),
            _ => Err(ApiError::BadRequest(
                "a `path` query parameter is required".to_owned(),
            )),
        }
    }
}

/// `GET /api/workdir/git` — detect whether a directory is a git repository.
///
/// Returns `{ repo_root, default_branch }`: `repo_root` is the repository root
/// containing `path` (`null` when it is not inside a git repository), and
/// `default_branch` is that repository's default branch when known. No fetch, so
/// this is cheap to call as the picker's selection changes. A missing `path` is
/// a `400`.
pub(crate) async fn workdir_git(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitRepoResponse>, ApiError> {
    let path = query.require_path()?;
    let info = state.interactor().git_repo_info(path).await?;
    Ok(Json(WireGitRepoResponse::from(info)))
}

/// `GET /api/workdir/git/branches` — the remote branches of a repository.
///
/// Resolves the repository containing `path`, fetches the remote, and returns
/// `{ default_branch, remote_branches }` so a branch picker can offer a base for
/// a worktree. A `path` that is not inside a git repository is a `400` (the
/// `not a git repository` use-case error), and a missing `path` is also a `400`.
pub(crate) async fn workdir_git_branches(
    State(state): State<AppState>,
    Query(query): Query<WorkdirGitQuery>,
) -> Result<Json<WireGitBranchesResponse>, ApiError> {
    let path = query.require_path()?;
    let remote = state.interactor().git_remote_branches(path).await?;
    Ok(Json(WireGitBranchesResponse::from(remote)))
}

/// `GET /api/launch-options` — the registered custom launch options.
///
/// Returns the flat `(label?, name, value?)` records the user has registered as
/// custom `claude` CLI flags, newest first, for the settings screen to list and
/// manage. Selecting which to apply when starting a session is a separate
/// concern handled elsewhere.
pub(crate) async fn list_launch_options(
    State(state): State<AppState>,
) -> Result<Json<WireLaunchOptionsResponse>, ApiError> {
    let options = state.interactor().list_launch_options().await?;
    Ok(Json(WireLaunchOptionsResponse {
        launch_options: options.into_iter().map(WireLaunchOption::from).collect(),
    }))
}

/// `POST /api/launch-options` — register a new custom launch option.
///
/// `name` (the flag) is required and must be non-blank; `label` and `value` are
/// optional (a valueless flag carries no `value`). A blank `name` is a `400`.
/// Returns the created record so the client can render it without a refetch.
pub(crate) async fn create_launch_option(
    State(state): State<AppState>,
    Json(req): Json<WireCreateLaunchOptionRequest>,
) -> Result<(StatusCode, Json<WireLaunchOption>), ApiError> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "a launch option must have a non-blank `name` (the flag)".to_owned(),
        ));
    }
    // `label`/`value` are kept verbatim apart from trimming surrounding
    // whitespace; an all-blank optional is treated as absent rather than a
    // stored empty string.
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let value = req
        .value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let option = state
        .interactor()
        .create_launch_option(label, name, value, req.default_enabled)
        .await?;
    Ok((StatusCode::CREATED, Json(WireLaunchOption::from(option))))
}

/// `PATCH /api/launch-options/{id}` — set a launch option's `default_enabled`
/// flag in place.
///
/// Updating in place preserves the option's id and `created_at` (a
/// delete+recreate would churn both); `name`, `value`, and `label` are immutable
/// through this endpoint. Returns the updated record so the client can render it
/// without a refetch, or `404` when no option has that id.
pub(crate) async fn update_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<WireUpdateLaunchOptionRequest>,
) -> Result<Json<WireLaunchOption>, ApiError> {
    let option = state
        .interactor()
        .set_launch_option_default_enabled(id, req.default_enabled)
        .await?;
    match option {
        Some(option) => Ok(Json(WireLaunchOption::from(option))),
        None => Err(ApiError::NotFound(format!(
            "no launch option with id {id}"
        ))),
    }
}

/// `DELETE /api/launch-options/{id}` — remove a registered launch option.
///
/// Deleting an unknown id is a no-op, so this is idempotent and always replies
/// `204`.
pub(crate) async fn delete_launch_option(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.interactor().delete_launch_option(id).await?;
    Ok(StatusCode::NO_CONTENT)
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

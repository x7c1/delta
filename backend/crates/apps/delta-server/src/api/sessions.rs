//! `/api/sessions` and `/api/threads` — listing, opening, closing and
//! removing sessions, and reading their threads, sends and messages.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use delta_usecase::{SessionId, ThreadId};
use delta_wire::rest::{
    WireMessagesResponse, WireSendsResponse, WireSessionListItem, WireSessionsResponse,
    WireThreadsResponse,
};

use crate::state::AppState;

use super::session_cursor;
use super::ApiError;

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

/// `GET /api/sessions` — one page of known sessions, open-first.
///
/// Returns a single page, each session tagged with whether it currently has a
/// live pane (`open`) and its `main` thread id, so the navigator can show and
/// route into every conversation — open or closed. Live sessions — open, or a
/// spawn still in flight — lead the list and closed sessions follow,
/// most-recently-active first within each group. The first page
/// carries every live session plus up to `limit` closed ones; later pages
/// carry closed sessions only, `limit` per page (default
/// [`DEFAULT_PAGE_LIMIT`], capped at [`MAX_PAGE_LIMIT`]). When more rows may
/// follow, `next_cursor` carries an opaque token the caller echoes back as
/// `cursor` to fetch the next page; a malformed `cursor` is a `400`.
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

/// `POST /api/sessions/{id}/open` — resume a closed, known session.
///
/// For a Claude session this re-launches `claude --resume <id>` and binds the
/// new pane; for a terminal-less Codex session it reconnects the adapter via
/// `thread/resume` (there is no pane). Either way it broadcasts `SessionOpened`,
/// and re-opening an already-open session is a no-op.
pub(crate) async fn open_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    state.interactor().open_session(&id).await?;
    state.broadcast([delta_usecase::SessionEvent::SessionOpened { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/close` — tear down a session's pane, keep its data;
/// or, for a session that is still starting, cancel its launch.
///
/// For a bound session it kills the live pane and drops it from the registry;
/// the conversation remains in the store and can be reopened. Closing settles
/// what the session strands: every permission request still awaiting an answer
/// is denied and broadcast as a `permission_resolved`, and any lingering
/// background subagent whose completion notification can no longer arrive is
/// swept, broadcasting a `subagent_finished` so live viewers' indicators clear
/// immediately.
///
/// A session that is **still starting** holds no conversation yet, so closing it
/// cancels the launch instead: whatever the launch stood up is reclaimed and the
/// eagerly-created row is marked `failed`, reported as a `SpawnFailed` marked
/// `cancelled` — this is the only producer that sets it — whose `reason` names
/// the close and is persisted on the row.
/// That event is in the returned batch, broadcast here like the sweep's.
///
/// `SessionClosed` is broadcast either way: what a client does on it is refetch
/// its session list (and that session's open sends), and for a cancelled launch
/// the row coming back `failed` from that refetch is exactly right. It goes out
/// *after* the batch above, so a `SpawnFailed` in it has already told the client
/// the launch is over — the session's own screen is then what explains it.
pub(crate) async fn close_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    let events = state.interactor().close_session(&id).await?;
    state.broadcast(events);
    state.broadcast([delta_usecase::SessionEvent::SessionClosed { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/sessions/{id}` — remove a closed session from Delta's list.
///
/// Deletes the `session` row and, by cascade, everything hanging off it, then
/// the git worktree Delta created for the session and its `delta-<id>` branch
/// **when they hold no work**: a worktree with modified or untracked files and
/// a branch that is not merged are kept, as is a branch the session was
/// started on rather than one Delta cut for it. The agent's own transcript and
/// state files are never touched. What was removed and what was kept is logged
/// at `info`; the response carries no body either way, and a kept item never
/// refuses the removal.
///
/// Only a session that is neither open nor still starting can be removed — the
/// use case refuses the other two states with `409` (`session_open` /
/// `session_spawning`) and touches no row or file when it does, and an unknown
/// id is a `404`. The browser only offers the action on a closed card, so
/// those refusals answer a stale card (another tab reopened the session)
/// rather than anything a user meets in normal use.
///
/// `SessionRemoved` is broadcast on success, not `SessionClosed`: every open tab
/// has to drop the row, which is not what closing means.
pub(crate) async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SessionId::from(id);
    let removal = state.interactor().delete_session(&id).await?;
    tracing::info!(session_id = %id, "removed session; {removal}");
    state.broadcast([delta_usecase::SessionEvent::SessionRemoved { session_id: id }]);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/sessions/{id}/interrupt` — abort a session's in-flight turn.
///
/// For a terminal-less agent (Codex) this drives the adapter's `interrupt`
/// without closing the session; the resulting `TurnInterrupted` settles over
/// the async event seam (the WebSocket broadcast), so — like a permission
/// decision or a question answer on a Codex session — no event is broadcast
/// synchronously here. For a pane-backed (Claude) or closed session it is a
/// well-defined no-op: Claude's turn interrupt is TUI-driven (Escape in the
/// pane).
pub(crate) async fn interrupt(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.interactor().interrupt(&SessionId::from(id)).await?;
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
/// pane, awaiting transcript correlation) — oldest first. A queued row may
/// carry `held_at`: it was recovered at boot from a dead process's
/// `dispatched` state and never auto-dispatches — the browser renders it
/// with explicit Send ([`release_send`]) and Cancel actions instead of the
/// waiting label. This is the source of truth for the browser's send strip.
/// An unknown session id is a `404`, so a removed session is
/// distinguishable from "nothing pending".
///
/// [`release_send`]: super::release_send
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

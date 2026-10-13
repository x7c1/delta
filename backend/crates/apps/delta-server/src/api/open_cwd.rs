//! `POST /api/open-cwd` — launching an external tool at a session's cwd.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::WireOpenCwdRequest;

use crate::state::AppState;

use super::ApiError;

/// `POST /api/open-cwd` — launch an external tool at a session's cwd.
///
/// Currently only VS Code is registered as a handler (spawned as
/// `code <path>`), but the request already carries an optional `handler` id
/// so a future addition can drop in without a breaking change. Defaults to
/// the `vscode` handler when `handler` is absent.
///
/// The request `path` MUST be a path Delta has already surfaced to the
/// browser (a `session.cwd`, `session.requested_workdir`, or `message.cwd`).
/// The interactor checks this allowlist before invoking the opener, so a
/// hand-crafted request cannot point the editor at an arbitrary directory
/// on disk. Replies `204 No Content` on a successful spawn; the browser
/// shows no toast on success — the editor opening is self-evident.
///
/// Error responses:
///
/// - `400` with code `open_cwd_path_not_allowed` — the path is not in the
///   allowlist. The click site never sends one, so this only fires against
///   a hand-crafted request.
/// - `400` with code `open_cwd_unknown_handler` — the `handler` id is not
///   registered. Same UX as above.
/// - `500` with code `open_cwd_command_not_found` — the tool binary
///   (`code`) is not installed on the server host. The browser renders a
///   specific "VS Code is not installed" message.
/// - `500` with code `open_cwd_spawn_failed` — any other spawn failure
///   (fork error, permission denied, etc.).
pub(crate) async fn open_cwd(
    State(state): State<AppState>,
    Json(req): Json<WireOpenCwdRequest>,
) -> Result<StatusCode, ApiError> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err(ApiError::BadRequest(
            "`path` must be a non-blank string".to_owned(),
        ));
    }
    state
        .interactor()
        .open_cwd(path, req.handler.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

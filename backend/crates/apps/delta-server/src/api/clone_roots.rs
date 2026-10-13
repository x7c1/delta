//! `/api/clone-roots` — the directories repositories are cloned into.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::{WireCloneRoot, WireCloneRootsResponse, WireCreateCloneRootRequest};

use crate::state::AppState;

use super::clone_root_path;
use super::ApiError;

/// `GET /api/clone-roots` — the registered clone roots.
///
/// Returns the directories the user has registered as homes for their git
/// clones, newest first. Each entry carries only the path; the stored
/// `created_at` is omitted from the wire because the Settings list does not
/// show it.
pub(crate) async fn list_clone_roots(
    State(state): State<AppState>,
) -> Result<Json<WireCloneRootsResponse>, ApiError> {
    let roots = state.interactor().list_clone_roots().await?;
    Ok(Json(WireCloneRootsResponse {
        clone_roots: roots.into_iter().map(WireCloneRoot::from).collect(),
    }))
}

/// `POST /api/clone-roots` — register a new clone root.
///
/// `path` must be a non-blank absolute path (starting with `/`). Trailing
/// slashes are trimmed for canonicalisation, so `/home/dev/projects/` and
/// `/home/dev/projects` register the same row. Blank input — empty,
/// whitespace-only, or nothing but slashes — is a `400`; the bare root `/` is
/// not blank and registers like any other path. The path is NOT required to
/// exist or to contain git repos at registration time — a future-state clone
/// root is allowed. Duplicate paths return `409` with code
/// `clone_root_duplicate` so the Settings dialog can show an inline hint.
pub(crate) async fn create_clone_root(
    State(state): State<AppState>,
    Json(req): Json<WireCreateCloneRootRequest>,
) -> Result<(StatusCode, Json<WireCloneRoot>), ApiError> {
    let trimmed = req.path.trim();
    // Strip trailing slashes so the user-typed form is canonicalised before the
    // PRIMARY KEY check sees it. Only the bare `/` is exempt: it is the one
    // all-slash spelling this contract takes as a deliberate root, while `"//"`
    // and `"///"` strip down to nothing, blank like `""` and `"   "`.
    // Canonicalising before the blankness check below is what rejects those
    // instead of quietly registering `/`.
    let canonical = if trimmed == "/" {
        "/"
    } else {
        trimmed.trim_end_matches('/')
    };
    if canonical.is_empty() {
        return Err(ApiError::BadRequest(
            "a clone root must have a non-blank `path`".to_owned(),
        ));
    }
    if !canonical.starts_with('/') {
        return Err(ApiError::BadRequest(
            "a clone root `path` must be absolute (start with `/`)".to_owned(),
        ));
    }
    let root = state.interactor().add_clone_root(canonical).await?;
    Ok((StatusCode::CREATED, Json(WireCloneRoot::from(root))))
}

/// `DELETE /api/clone-roots/{path_b64}` — unregister a clone root.
///
/// The registered absolute path is URL-safe base64 in the path segment to
/// keep its embedded `/` characters out of the route match. A malformed token
/// is a `400`; an unknown path is a silent no-op (idempotent), so a
/// Settings dialog click never surfaces a 404 noise on a path the user just
/// removed via another tab.
pub(crate) async fn delete_clone_root(
    State(state): State<AppState>,
    Path(path_b64): Path<String>,
) -> Result<StatusCode, ApiError> {
    let path = clone_root_path::decode(&path_b64)
        .ok_or_else(|| ApiError::BadRequest("malformed clone-root path token".to_owned()))?;
    state.interactor().remove_clone_root(&path).await?;
    Ok(StatusCode::NO_CONTENT)
}

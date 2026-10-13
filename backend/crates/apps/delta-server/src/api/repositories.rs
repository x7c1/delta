//! `/api/repositories` — the registered repositories, and cloning one.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::{WireCloneRepositoryRequest, WireRepositoriesResponse, WireRepositoryEntry};

use crate::state::AppState;

use super::ApiError;

/// `GET /api/repositories` — registered repositories for the new-session
/// Repository tab, ordered by the most recent activity across each
/// repository's clones.
///
/// Aggregates the session history: every distinct (repo_root, clone_path)
/// pair becomes a clone, and clones whose `git config --get
/// remote.origin.url` collapses to the same normalised key bundle under one
/// repository. Clones whose path no longer exists on disk are filtered out
/// (lazy GC); a repository drained of every clone disappears too. Sessions
/// launched outside any git repo do not contribute — the Recent dirs list
/// (Directory tab) is where those surface.
pub(crate) async fn list_repositories(
    State(state): State<AppState>,
) -> Result<Json<WireRepositoriesResponse>, ApiError> {
    let repositories = state.interactor().list_repositories().await?;
    Ok(Json(WireRepositoriesResponse {
        repositories: repositories
            .into_iter()
            .map(WireRepositoryEntry::from)
            .collect(),
    }))
}

/// `POST /api/repositories/clone` — clone a repository into a registered clone
/// root.
///
/// Accepts (`202`) and runs the clone as a background job: cloning takes far
/// longer than a request should, so the outcome arrives on `/ws` as
/// `repository_clone_completed` / `repository_clone_failed` rather than in this
/// response. The refusals happen here, before any job exists: an unregistered
/// `clone_root` is a `400` with code `clone_root_not_registered`, and an
/// already-occupied `<clone_root>/<repo_name>` is a `409` with code
/// `clone_dest_exists` — there is no fallback naming, so Delta refuses rather
/// than cloning next to it. A second request for a destination already being
/// cloned joins that job (also `202`) instead of starting a second `gh`.
pub(crate) async fn clone_repository(
    State(state): State<AppState>,
    Json(req): Json<WireCloneRepositoryRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .interactor()
        .clone_repository(&req.repo_owner, &req.repo_name, &req.clone_root)
        .await?;
    Ok(StatusCode::ACCEPTED)
}

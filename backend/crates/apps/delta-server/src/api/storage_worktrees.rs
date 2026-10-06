//! `GET` and `DELETE /api/storage/worktrees` — the directories under the
//! worktree base, and removing one no session works in.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::{
    WireRemoveWorktreeRequest, WireStorageWorktree, WireStorageWorktreesResponse,
};

use super::ApiError;
use crate::state::AppState;

/// `GET /api/storage/worktrees` — every directory under the worktree base,
/// with whether a listed session works in it and what git says of it.
pub(crate) async fn list_orphan_worktrees(
    State(state): State<AppState>,
) -> Result<Json<WireStorageWorktreesResponse>, ApiError> {
    let dirs = state.interactor().list_worktree_dirs().await?;
    Ok(Json(WireStorageWorktreesResponse {
        worktrees: dirs.into_iter().map(WireStorageWorktree::from).collect(),
    }))
}

/// `DELETE /api/storage/worktrees` — remove one directory under the worktree
/// base that no listed session works in (`204`). The refusals are `409`s with
/// their own codes; see `delta_usecase::InteractorCore::remove_worktree_dir`.
pub(crate) async fn remove_orphan_worktree(
    State(state): State<AppState>,
    Json(request): Json<WireRemoveWorktreeRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .interactor()
        .remove_worktree_dir(&request.path, request.force.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

//! `DELETE /api/storage/snapshots` — deleting a migration snapshot.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_wire::rest::WireDeleteSnapshotRequest;

use super::ApiError;
use crate::state::AppState;
use crate::storage_inventory::SnapshotDeletionError;

/// `DELETE /api/storage/snapshots` — delete one of the migration snapshots
/// `GET /api/storage` lists (`204`). A path that is not one of them is a
/// `404`, and nothing is deleted.
pub(crate) async fn delete_snapshot(
    State(state): State<AppState>,
    Json(request): Json<WireDeleteSnapshotRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .storage()
        .delete_snapshot(&request.path)
        .map_err(|err| match err {
            SnapshotDeletionError::NotListed(_) => ApiError::NotFound(err.to_string()),
            SnapshotDeletionError::Io { .. } => ApiError::Internal(err.to_string()),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

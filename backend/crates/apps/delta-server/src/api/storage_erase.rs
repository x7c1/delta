//! `POST /api/storage/erase` — erasing everything this Delta created on the
//! machine that holds no work, then stopping the server.

use std::time::SystemTime;

use axum::extract::State;
use axum::Json;

use delta_wire::rest::WireEraseResponse;

use super::ApiError;
use crate::serve::ServerStopped;
use crate::state::AppState;

/// `POST /api/storage/erase` — run `Interactor::erase_everything`, delete the
/// derived files in the data directory, answer with the report, and ask the
/// server to stop; once it has, it deletes the rest of the data directory
/// (see `serve::serve`). A second erase while one runs is `409`
/// `erase_in_progress`.
pub(crate) async fn erase_everything(
    State(state): State<AppState>,
) -> Result<Json<WireEraseResponse>, ApiError> {
    if !state.begin_erase() {
        return Err(ApiError::EraseInProgress);
    }
    let storage = state.storage();
    let report = match state
        .interactor()
        .erase_everything(SystemTime::now(), storage.worktree_base_parent())
        .await
    {
        Ok(report) => report,
        Err(err) => {
            state.abandon_erase();
            return Err(err.into());
        }
    };
    storage.delete_derived_files();
    let response = WireEraseResponse::new(report.clone(), storage.data_dir());
    // Graceful: this response is still sent before the server stops.
    state.stop(ServerStopped::Erased(report));
    Ok(Json(response))
}

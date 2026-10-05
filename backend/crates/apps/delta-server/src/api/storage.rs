//! `GET /api/storage` — where this running Delta keeps its files.

use axum::extract::State;
use axum::Json;

use delta_wire::rest::WireStorageResponse;

use crate::state::AppState;

/// `GET /api/storage` — the storage inventory, with sizes read now.
///
/// `version` is the same string `GET /api/version` returns, so the Settings
/// category can say which build it describes without a second request. See
/// [`crate::StorageInventory::report`] for how sizes are read.
pub(crate) async fn get_storage(State(state): State<AppState>) -> Json<WireStorageResponse> {
    Json(
        state
            .storage()
            .report(state.tmux_socket(), crate::version::display_version()),
    )
}

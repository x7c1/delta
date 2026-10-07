//! `GET /api/latest-release` — whether a newer release of Delta is out.

use axum::extract::State;
use axum::Json;

use delta_wire::rest::WireLatestReleaseResponse;

use crate::state::AppState;

/// `GET /api/latest-release` — a published release newer than this server,
/// from the last background check (`null` when there is nothing to tell).
///
/// Answers from the state alone: the request never reaches GitHub, and a
/// failed check never surfaces here as an error.
pub(crate) async fn get_latest_release(
    State(state): State<AppState>,
) -> Json<WireLatestReleaseResponse> {
    Json(WireLatestReleaseResponse {
        newer: state.newer_release().map(Into::into),
    })
}

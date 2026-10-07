//! `GET /api/latest-release` — whether a newer release of Delta is out — and
//! `POST /api/latest-release/download` — downloading it.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_usecase::UpdateDownload;
use delta_wire::rest::{WireLatestReleaseResponse, WireUpdateDownload};

use super::ApiError;
use crate::state::AppState;

/// `GET /api/latest-release` — a published release newer than this server,
/// from the last background check (`null` when there is nothing to tell),
/// what the browser may offer next to it, and how its download is going.
///
/// Answers from the state alone: the request never reaches GitHub, and a
/// failed check never surfaces here as an error.
pub(crate) async fn get_latest_release(
    State(state): State<AppState>,
) -> Json<WireLatestReleaseResponse> {
    let newer = state.newer_release();
    let update = state.release_update();
    Json(WireLatestReleaseResponse {
        download: update.download_of(newer.as_ref()).map(Into::into),
        offer: update.offer(newer.as_ref()).into(),
        newer: newer.map(Into::into),
    })
}

/// `POST /api/latest-release/download` — start downloading the newer
/// release's asset for this platform, or report the download already running
/// (`202`) or done (`200`).
///
/// Every refusal is the server's own: a `409` with a stable code when the CLI
/// launched the server, the build is local, its HTTPS client could not be set
/// up, no newer release is known, or the release has no asset this platform
/// may download.
pub(crate) async fn download_latest_release(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<WireUpdateDownload>), ApiError> {
    let download = state
        .release_update()
        .start(state.newer_release())
        .map_err(ApiError::UpdateRefused)?;
    let status = match download {
        UpdateDownload::Ready { .. } => StatusCode::OK,
        UpdateDownload::Downloading { .. } | UpdateDownload::Failed { .. } => StatusCode::ACCEPTED,
    };
    Ok((status, Json(download.into())))
}

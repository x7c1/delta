//! `GET /api/latest-release` — whether a newer release of Delta is out — and
//! `POST /api/latest-release/{download,install,restart}` — downloading it,
//! installing it, and restarting into it.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use delta_usecase::{UpdateDownload, UpdateInstall};
use delta_wire::rest::{WireLatestReleaseResponse, WireUpdateDownload, WireUpdateInstall};

use super::ApiError;
use crate::serve::ServerStopped;
use crate::state::AppState;

/// `GET /api/latest-release` — a published release newer than this server,
/// from the last background check (`null` when there is nothing to tell),
/// what the browser may offer next to it, and how its download and install
/// are going.
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
        installs: update.installs(),
        install: update.install_of(newer.as_ref()).map(Into::into),
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

/// `POST /api/latest-release/install` — start installing the verified
/// download of the newer release, or report the install already running
/// (`202`) or done (`200`).
///
/// The file and the version are the ready download's; the request carries
/// nothing. Every refusal is a `409` with a stable code: the CLI launched the
/// server, the build is local, the app installs nothing itself on this
/// platform, or no verified download of the newer release is ready.
pub(crate) async fn install_latest_release(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<WireUpdateInstall>), ApiError> {
    let install = state
        .release_update()
        .install(state.newer_release())
        .map_err(ApiError::UpdateRefused)?;
    let status = match install {
        UpdateInstall::Installed { .. } => StatusCode::OK,
        UpdateInstall::Installing { .. }
        | UpdateInstall::Rejected { .. }
        | UpdateInstall::Failed { .. }
        | UpdateInstall::Unavailable { .. } => StatusCode::ACCEPTED,
    };
    Ok((status, Json(install.into())))
}

/// `POST /api/latest-release/restart` — once an update is installed, answer
/// `204` and stop the server for the restart ([`ServerStopped::Restart`]);
/// the desktop shell then starts the installed app again. A `409`
/// `update_not_installed` otherwise.
pub(crate) async fn restart_latest_release(
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let version = state
        .release_update()
        .restart()
        .map_err(ApiError::UpdateRefused)?;
    // Graceful: this response is still sent before the server stops.
    state.stop(ServerStopped::Restart { version });
    Ok(StatusCode::NO_CONTENT)
}

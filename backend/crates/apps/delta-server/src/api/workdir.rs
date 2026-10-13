//! `/api/workdir/list` and `/api/workdir/recent` — the working-directory picker.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use delta_wire::rest::{WireRecentWorkdirItem, WireWorkdirListResponse, WireWorkdirRecentResponse};

use crate::state::AppState;

use super::ApiError;

/// Query parameters for `GET /api/workdir/list`: the directory to browse and
/// whether to include dot-directories.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkdirListQuery {
    /// The absolute path to list. Omitted or empty defaults to the user's home
    /// directory, so the picker has a sensible starting point.
    #[serde(default)]
    path: Option<String>,
    /// `?hidden=true` also lists dot-directories. Omitted means `false`, so
    /// dot-directories stay hidden by default.
    #[serde(default, rename = "hidden")]
    include_hidden: bool,
}

/// `GET /api/workdir/list` — browse a directory for the working-directory picker.
///
/// Lists the immediate subdirectories of `path` (dirs only, sorted by name),
/// along with the canonical path and its parent so the picker can step up.
/// Dot-directories are hidden unless `?hidden=true` is given. `path` defaults
/// to `$HOME` when omitted. A missing path or a non-directory is a `400`; a
/// permission error is a `403`.
pub(crate) async fn list_workdir(
    State(state): State<AppState>,
    Query(query): Query<WorkdirListQuery>,
) -> Result<Json<WireWorkdirListResponse>, ApiError> {
    let listing = state
        .interactor()
        .browse_workdir(query.path.as_deref(), query.include_hidden)
        .await?;
    Ok(Json(WireWorkdirListResponse::from(listing)))
}

/// `GET /api/workdir/recent` — recently-used working directories for the picker.
///
/// Returns the distinct directories sessions have run in, most-recently-used
/// first, derived from existing session rows (Delta keeps no separate history).
pub(crate) async fn recent_workdir(
    State(state): State<AppState>,
) -> Result<Json<WireWorkdirRecentResponse>, ApiError> {
    let workdirs = state.interactor().recent_workdirs().await?;
    Ok(Json(WireWorkdirRecentResponse {
        workdirs: workdirs
            .into_iter()
            .map(WireRecentWorkdirItem::from)
            .collect(),
    }))
}

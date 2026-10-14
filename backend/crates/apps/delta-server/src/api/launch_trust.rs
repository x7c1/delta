//! `/api/launch-trust` — whether a new worktree session needs the user's
//! confirmation before Claude Code's trust is pre-accepted for it.

use axum::extract::State;
use axum::Json;

use delta_wire::rest::{WireLaunchTrustRequest, WireLaunchTrustResponse};

use crate::state::AppState;

use super::ApiError;

/// `POST /api/launch-trust` — check a new-session worktree launch against the
/// launch-trust rule before it is sent.
///
/// Answers `{ needs_confirmation, repository_untrusted, commit,
/// agent_config_changes }`. Creates no worktree and no session; for a remote
/// start point the branch is fetched first, as the launch itself would. A
/// `workdir` that does not exist or is not inside a git repository is a `400`,
/// as it is on the send.
pub(crate) async fn check_launch_trust(
    State(state): State<AppState>,
    Json(req): Json<WireLaunchTrustRequest>,
) -> Result<Json<WireLaunchTrustResponse>, ApiError> {
    let (workdir, start_point, provider) = req.into_parts();
    let check = state
        .interactor()
        .check_launch_trust(&workdir, &start_point, provider)
        .await?;
    Ok(Json(WireLaunchTrustResponse::from(check)))
}

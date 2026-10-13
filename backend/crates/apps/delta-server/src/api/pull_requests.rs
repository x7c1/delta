//! `GET /api/prs` — pull requests for the new-session picker.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use delta_usecase::PullRequestLens;
use delta_wire::rest::WirePullRequestsResponse;

use crate::state::AppState;

use super::ApiError;

/// Query parameters for `GET /api/prs`: which lens to query gh for.
#[derive(Debug, Deserialize)]
pub(crate) struct ListPullRequestsQuery {
    /// The lens name (`reviewer` or `author`). Required: there is no
    /// sensible default lens — the PR tab asks for one explicitly per
    /// section.
    lens: String,
}

/// `GET /api/prs?lens=reviewer|author` — pull requests for the new-session
/// PR tab.
///
/// Drives the PR search through the gateway, then joins the result
/// against the registered repositories so each row carries
/// `has_local_clone`. When `gh` is not installed or `gh auth status`
/// fails, the response is `{ gh_available: false, pull_requests: [] }`
/// at 200 — the PR tab renders an inline "run `gh auth login`" hint
/// rather than a generic failure. An unknown `lens` is a `400`.
pub(crate) async fn list_pull_requests(
    State(state): State<AppState>,
    Query(query): Query<ListPullRequestsQuery>,
) -> Result<Json<WirePullRequestsResponse>, ApiError> {
    let lens = PullRequestLens::parse(&query.lens).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "unknown lens '{}': expected 'reviewer' or 'author'",
            query.lens
        ))
    })?;
    let list = state.interactor().list_pull_requests(lens).await?;
    Ok(Json(WirePullRequestsResponse::from(list)))
}

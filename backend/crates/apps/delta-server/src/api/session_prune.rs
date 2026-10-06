//! `GET` and `POST /api/sessions/prune` — previewing and running the bulk
//! removal of old closed sessions.

use std::time::SystemTime;

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use delta_usecase::{PruneCriteria, PruneStatus, SessionEvent};
use delta_wire::rest::{
    WirePrunePreviewResponse, WirePruneSessionsRequest, WirePruneSessionsResponse, WirePruneStatus,
};

use super::ApiError;
use crate::state::AppState;

/// Query parameters for `GET /api/sessions/prune`: the body of
/// `POST /api/sessions/prune`, with `statuses` as a comma-separated list.
#[derive(Debug, Deserialize)]
pub(crate) struct PrunePreviewQuery {
    older_than_days: u32,
    /// `ended`, `failed`, or both comma-separated; absent means both. An empty
    /// value matches nothing, as an empty list in the body does.
    statuses: Option<String>,
}

impl PrunePreviewQuery {
    /// The criteria this query names, or a `400` for an unknown status.
    fn criteria(self) -> Result<PruneCriteria, ApiError> {
        let statuses = match self.statuses {
            None => PruneStatus::ALL.to_vec(),
            Some(list) => list
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| {
                    WirePruneStatus::parse(value)
                        .map(PruneStatus::from)
                        .ok_or_else(|| ApiError::BadRequest(format!("unknown status: {value}")))
                })
                .collect::<Result<_, _>>()?,
        };
        Ok(PruneCriteria {
            older_than_days: self.older_than_days,
            statuses,
        })
    }
}

/// `GET /api/sessions/prune` — the sessions the bulk removal would take now,
/// without removing anything. A missing or negative `older_than_days`, or an
/// unknown status, is a `400`.
pub(crate) async fn preview_prune_sessions(
    State(state): State<AppState>,
    Query(query): Query<PrunePreviewQuery>,
) -> Result<Json<WirePrunePreviewResponse>, ApiError> {
    let criteria = query.criteria()?;
    let candidates = state
        .interactor()
        .prune_candidates(&criteria, SystemTime::now())
        .await?;
    Ok(Json(WirePrunePreviewResponse::new(
        candidates.iter().map(|id| id.as_str().to_owned()).collect(),
    )))
}

/// `POST /api/sessions/prune` — remove every matching closed session, each as
/// `DELETE /api/sessions/{id}` would, and broadcast `session_removed` for
/// each one removed.
pub(crate) async fn prune_sessions(
    State(state): State<AppState>,
    Json(request): Json<WirePruneSessionsRequest>,
) -> Result<Json<WirePruneSessionsResponse>, ApiError> {
    let criteria = PruneCriteria::from(request);
    let report = state
        .interactor()
        .prune_sessions(&criteria, SystemTime::now())
        .await?;
    tracing::info!(
        removed = report.removed.len(),
        skipped = report.skipped.len(),
        kept = report.kept.len(),
        older_than_days = criteria.older_than_days,
        "pruned old sessions"
    );
    state.broadcast(
        report
            .removed
            .iter()
            .map(|session_id| SessionEvent::SessionRemoved {
                session_id: session_id.clone(),
            }),
    );
    Ok(Json(WirePruneSessionsResponse::from(report)))
}
